#import "IconPacks.h"

#import "FoundationJSON.h"
#import "AstralState.h"
#import "SharedPacks.h"

static NSCache<NSString *, UIImage *> *gIcons;

NSArray<NSDictionary *> *AstralIconPacks(void)
{
    return AstralObjectFromJSON(AstralSharedPacks().icons());
}

NSString *AstralActiveIconPackID(void)
{
    if (!AstralSettings()) return @"discord";
    return [NSString stringWithUTF8String:AstralSharedPacks().activeIcons().c_str()];
}

void AstralSelectIconPack(NSString *identifier)
{
    AstralSharedPacks().selectIcons(identifier.UTF8String ?: "");
    @synchronized(NSCache.class) {
        [gIcons removeAllObjects];
    }
}

UIImage *AstralIconPackImage(NSString *name)
{
    if (!AstralSettings()) return nil;
    @synchronized(NSCache.class) {
        if (!gIcons) gIcons = [NSCache new];
        std::string path = AstralSharedPacks().iconPath(name.UTF8String ?: "");
        if (path.empty()) return nil;
        NSString *key = [NSString stringWithUTF8String:path.c_str()];
        UIImage *cached = [gIcons objectForKey:key];
        if (cached) return cached;
        CGFloat scale = [key hasSuffix:@"@3x.png"] ? 3 : [key hasSuffix:@"@2x.png"] ? 2 : 1;
        NSData *data = [NSData dataWithContentsOfFile:key];
        UIImage *image = data ? [[UIImage imageWithData:data scale:scale] imageWithRenderingMode:UIImageRenderingModeAlwaysOriginal] : nil;
        if (image) [gIcons setObject:image forKey:key];
        return image;
    }
}
