#import "ImageAssets.h"

#import <UIKit/UIKit.h>
#import <objc/runtime.h>

#import "AstralState.h"

static NSString *const kAssetPrefix = @"astral_";

static UIImage *(*originalNamedInBundleTraits)(id, SEL, NSString *, NSBundle *, UITraitCollection *);
static UIImage *(*originalNamedInBundleConfiguration)(id, SEL, NSString *, NSBundle *, UIImageConfiguration *);
static NSCache<NSString *, UIImage *> *gImages;

static UIImage *AstralImage(NSString *name)
{
    if (![name hasPrefix:kAssetPrefix]) return nil;
    UIImage *cached = [gImages objectForKey:name];
    if (cached) return cached;
    NSString *file = [name substringFromIndex:kAssetPrefix.length];
    if ([file containsString:@"/"] || [file containsString:@".."]) return nil;
    NSString *assets = [AstralResourceDirectory() stringByAppendingPathComponent:@"assets"];
    NSString *path = [assets stringByAppendingPathComponent:[file stringByAppendingString:@"@3x.png"]];
    UIImage *image = [UIImage imageWithContentsOfFile:path];
    if (!image) return nil;
    image = [[UIImage imageWithCGImage:image.CGImage scale:3 orientation:UIImageOrientationUp] imageWithRenderingMode:UIImageRenderingModeAlwaysTemplate];
    [gImages setObject:image forKey:name];
    return image;
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
    Replace(@selector(imageNamed:inBundle:compatibleWithTraitCollection:), (IMP)NamedInBundleTraits, (IMP *)&originalNamedInBundleTraits);
    Replace(@selector(imageNamed:inBundle:withConfiguration:), (IMP)NamedInBundleConfiguration, (IMP *)&originalNamedInBundleConfiguration);
}
