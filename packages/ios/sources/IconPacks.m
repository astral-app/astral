#import "IconPacks.h"

#import "AstralCore.h"

static NSCache<NSString *, UIImage *> *Icons(void)
{
    static NSCache<NSString *, UIImage *> *icons;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        icons = [NSCache new];
    });
    return icons;
}

static CGFloat Scale(NSString *path)
{
    if ([path hasSuffix:@"@3x.png"]) return 3;
    if ([path hasSuffix:@"@2x.png"]) return 2;
    return 1;
}

UIImage *AstralIconPackImage(NSString *name)
{
    NSString *path = AstralTakeString(astral_ios_icon_path(AstralTextOf(name)));
    if (!path.length) return nil;
    UIImage *cached = [Icons() objectForKey:path];
    if (cached) return cached;
    NSData *data = [NSData dataWithContentsOfFile:path];
    UIImage *image =
        data ? [[UIImage imageWithData:data scale:Scale(path)] imageWithRenderingMode:UIImageRenderingModeAlwaysOriginal] : nil;
    if (image) [Icons() setObject:image forKey:path];
    return image;
}
