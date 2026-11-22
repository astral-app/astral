#import "ImageAssets.h"

#import <UIKit/UIKit.h>
#import <objc/runtime.h>

#import "AstralState.h"
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

static UIImage *AstralImage(NSString *name)
{
    if (![name hasPrefix:kAssetPrefix]) return nil;
    UIImage *cached = [gImages objectForKey:name];
    if (cached) return cached;
    [gLock lock];
    NSString *svg = gRegistered[name];
    [gLock unlock];
    if (!svg) svg = BundledSVG([name substringFromIndex:kAssetPrefix.length]);
    UIImage *image = svg ? AstralRenderSVG(svg, kIconPoints) : nil;
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

static UIImage *NamedInBundleTraits(id self, SEL selector, NSString *name, NSBundle *bundle, UITraitCollection *traits)
{
    return AstralImage(name) ?: originalNamedInBundleTraits(self, selector, name, bundle, traits);
}

static UIImage *NamedInBundleConfiguration(id self, SEL selector, NSString *name, NSBundle *bundle, UIImageConfiguration *configuration)
{
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
