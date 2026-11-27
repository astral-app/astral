#import "ImageAssets.h"

#import <UIKit/UIKit.h>
#import <objc/runtime.h>

#import "AstralState.h"
#import "IconPacks.h"
#import "SvgRaster.h"

static NSString *const kAssetPrefix = @"astral_";
static const CGFloat kIconPoints = 24;

static UIImage *(*originalNamedInBundleTraits)(id, SEL, NSString *, NSBundle *, UITraitCollection *);
static UIImage *(*originalNamedInBundleConfiguration)(id, SEL, NSString *, NSBundle *, UIImageConfiguration *);
static NSCache<NSString *, UIImage *> *gImages;
static NSMutableDictionary<NSString *, NSString *> *gRegistered;
static NSLock *gLock;

static NSString *BundledSVG(NSString *file)
{
    if ([file containsString:@"/"] || [file containsString:@".."]) return nil;
    NSString *assets = [AstralResourceDirectory() stringByAppendingPathComponent:@"assets"];
    return [NSString stringWithContentsOfFile:[assets stringByAppendingPathComponent:[file stringByAppendingPathExtension:@"svg"]]
                                     encoding:NSUTF8StringEncoding
                                        error:nil];
}

static UIImage *BundledPNG(NSString *file)
{
    if ([file containsString:@"/"] || [file containsString:@".."]) return nil;
    NSString *assets = [AstralResourceDirectory() stringByAppendingPathComponent:@"assets"];
    UIImage *full = [UIImage imageWithContentsOfFile:[assets stringByAppendingPathComponent:[file stringByAppendingPathExtension:@"png"]]];
    if (!full) return nil;
    CGRect bounds = CGRectMake(0, 0, kIconPoints, kIconPoints);
    UIGraphicsImageRenderer *renderer = [[UIGraphicsImageRenderer alloc] initWithSize:bounds.size
                                                                               format:[UIGraphicsImageRendererFormat preferredFormat]];
    UIImage *scaled = [renderer imageWithActions:^(__unused UIGraphicsImageRendererContext *context) {
        [full drawInRect:bounds];
    }];
    return [scaled imageWithRenderingMode:UIImageRenderingModeAlwaysOriginal];
}

static NSString *const kTintedPrefix = @"astral_tinted_";

static UITraitCollection *gAppTraits;

static UITraitCollection *AppTraits(void)
{
    if (NSThread.isMainThread) {
        for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
            if (![scene isKindOfClass:UIWindowScene.class]) continue;
            for (UIWindow *window in ((UIWindowScene *)scene).windows) {
                if (window.isKeyWindow || !gAppTraits) gAppTraits = window.traitCollection;
            }
        }
    }
    return gAppTraits ?: [UITraitCollection traitCollectionWithUserInterfaceStyle:UIUserInterfaceStyleDark];
}

static UIColor *TintColor(NSString *code)
{
    if ([code isEqualToString:@"label"]) return [UIColor.labelColor resolvedColorWithTraitCollection:AppTraits()];
    unsigned value = 0;
    if (code.length != 6 || ![[NSScanner scannerWithString:code] scanHexInt:&value]) return nil;
    return [UIColor colorWithRed:((value >> 16) & 0xFF) / 255.0 green:((value >> 8) & 0xFF) / 255.0 blue:(value & 0xFF) / 255.0 alpha:1];
}

static UIImage *TintedImage(NSString *name)
{
    NSString *rest = [name substringFromIndex:kTintedPrefix.length];
    NSRange separator = [rest rangeOfString:@"_"];
    if (separator.location == NSNotFound) return nil;
    UIColor *color = TintColor([rest substringToIndex:separator.location]);
    NSString *svg = BundledSVG([rest substringFromIndex:separator.location + 1]);
    UIImage *symbol = color && svg ? AstralRenderSVG(svg, kIconPoints) : nil;
    if (!symbol) return nil;
    UIGraphicsImageRendererFormat *format = [UIGraphicsImageRendererFormat preferredFormat];
    format.scale = symbol.scale;
    UIGraphicsImageRenderer *renderer = [[UIGraphicsImageRenderer alloc] initWithSize:symbol.size format:format];
    UIImage *tinted = [renderer imageWithActions:^(__unused UIGraphicsImageRendererContext *context) {
        [color setFill];
        [[symbol imageWithRenderingMode:UIImageRenderingModeAlwaysTemplate] drawInRect:(CGRect){CGPointZero, symbol.size}];
        UIRectFillUsingBlendMode((CGRect){CGPointZero, symbol.size}, kCGBlendModeSourceIn);
    }];
    return [tinted imageWithRenderingMode:UIImageRenderingModeAlwaysOriginal];
}

static UIImage *AstralImage(NSString *name)
{
    if (![name hasPrefix:kAssetPrefix]) return AstralIconPackImage(name);
    if ([name hasPrefix:kTintedPrefix]) {
        UIImage *cached = [gImages objectForKey:name];
        if (cached) return cached;
        UIImage *tinted = TintedImage(name);
        if (tinted) [gImages setObject:tinted forKey:name];
        return tinted;
    }
    UIImage *packed = AstralIconPackImage(name);
    if (packed) return packed;
    UIImage *cached = [gImages objectForKey:name];
    if (cached) return cached;
    [gLock lock];
    NSString *svg = gRegistered[name];
    [gLock unlock];
    NSString *file = [name substringFromIndex:kAssetPrefix.length];
    if (!svg) svg = BundledSVG(file);
    UIImage *image = svg ? AstralRenderSVG(svg, kIconPoints) : BundledPNG(file);
    if (image) [gImages setObject:image forKey:name];
    return image;
}

NSString *AstralRegisterIconSVG(NSString *name, NSString *svg)
{
    NSString *asset = [kAssetPrefix stringByAppendingString:name];
    [gLock lock];
    if (![gRegistered[asset] isEqualToString:svg]) {
        gRegistered[asset] = svg;
        [gImages removeObjectForKey:asset];
    }
    [gLock unlock];
    return [@"res://" stringByAppendingString:asset];
}

static NSString *WithoutGeneration(NSString *name)
{
    NSRange mark = [name rangeOfString:@"#"];
    return mark.location == NSNotFound ? name : [name substringToIndex:mark.location];
}

static UIImage *NamedInBundleTraits(id self, SEL selector, NSString *name, NSBundle *bundle, UITraitCollection *traits)
{
    name = WithoutGeneration(name);
    return AstralImage(name) ?: originalNamedInBundleTraits(self, selector, name, bundle, traits);
}

static UIImage *NamedInBundleConfiguration(id self, SEL selector, NSString *name, NSBundle *bundle, UIImageConfiguration *configuration)
{
    name = WithoutGeneration(name);
    return AstralImage(name) ?: originalNamedInBundleConfiguration(self, selector, name, bundle, configuration);
}

static void Replace(SEL selector, IMP replacement, IMP *original)
{
    Method method = class_getClassMethod(UIImage.class, selector);
    if (method) *original = method_setImplementation(method, replacement);
}

void AstralInstallImageAssets(void)
{
    if (gImages) return;
    gImages = [NSCache new];
    gRegistered = [NSMutableDictionary dictionary];
    gLock = [NSLock new];
    Replace(@selector(imageNamed:inBundle:compatibleWithTraitCollection:), (IMP)NamedInBundleTraits, (IMP *)&originalNamedInBundleTraits);
    Replace(@selector(imageNamed:inBundle:withConfiguration:), (IMP)NamedInBundleConfiguration, (IMP *)&originalNamedInBundleConfiguration);
}
