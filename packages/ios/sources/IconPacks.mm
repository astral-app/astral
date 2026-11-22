#import "IconPacks.h"

#include <string>

#import "AstralState.h"
#import "TomlDocument.h"
#include "AstralSettings.h"

static NSString *const kNoIconPack = @"discord";
static const NSUInteger kScales[] = {3, 2, 1};

static NSCache<NSString *, id> *gIcons;
static NSString *gLoadedPack;
static NSString *gLoadedDirectory;

static NSString *LocalIconPacksDirectory(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"icon-packs"];
}

static NSDictionary *LoadIconPack(NSString *directory, BOOL bundled)
{
    NSDictionary *manifest = AstralParseTOML([NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:@"manifest.toml"]]);
    if (![manifest[@"id"] isKindOfClass:NSString.class] || ![manifest[@"name"] isKindOfClass:NSString.class]) return nil;
    NSMutableDictionary *pack = [manifest mutableCopy];
    pack[@"directory"] = directory;
    pack[@"bundled"] = @(bundled);
    pack[@"count"] = @([[NSFileManager.defaultManager contentsOfDirectoryAtPath:[directory stringByAppendingPathComponent:@"icons"]
                                                                          error:nil] count]);
    return pack;
}

NSArray<NSDictionary *> *AstralIconPacks(void)
{
    NSMutableArray *packs = [NSMutableArray arrayWithObject:@{
        @"id" : kNoIconPack,
        @"name" : @"Discord",
        @"description" : @"Discord's own icons",
        @"bundled" : @YES
    }];
    NSMutableSet *seen = [NSMutableSet setWithObject:kNoIconPack];
    NSString *bundled = [AstralResourceDirectory() stringByAppendingPathComponent:@"icon-packs"];
    for (NSString *root in @[ LocalIconPacksDirectory(), bundled ]) {
        for (NSString *entry in
             [[NSFileManager.defaultManager contentsOfDirectoryAtPath:root error:nil] sortedArrayUsingSelector:@selector(compare:)]) {
            NSDictionary *pack = LoadIconPack([root stringByAppendingPathComponent:entry], root == bundled);
            if (!pack || [seen containsObject:pack[@"id"]]) continue;
            [seen addObject:pack[@"id"]];
            [packs addObject:pack];
        }
    }
    return packs;
}

NSString *AstralActiveIconPackID(void)
{
    if (!AstralSettings()) return kNoIconPack;
    std::string active = AstralSettings()->getString("astral", "icon-pack", kNoIconPack.UTF8String);
    return [NSString stringWithUTF8String:active.c_str()] ?: kNoIconPack;
}

void AstralSelectIconPack(NSString *identifier)
{
    if (AstralSettings()->set("astral", "icon-pack", std::string(identifier.UTF8String))) AstralSettings()->save();
    @synchronized(NSCache.class) {
        gLoadedPack = nil;
        gLoadedDirectory = nil;
        [gIcons removeAllObjects];
    }
}

static NSString *ActiveDirectory(void)
{
    NSString *active = AstralActiveIconPackID();
    if ([active isEqualToString:kNoIconPack]) return nil;
    if ([gLoadedPack isEqualToString:active]) return gLoadedDirectory;
    gLoadedPack = active;
    gLoadedDirectory = nil;
    for (NSDictionary *pack in AstralIconPacks()) {
        if ([pack[@"id"] isEqualToString:active]) gLoadedDirectory = pack[@"directory"];
    }
    return gLoadedDirectory;
}

UIImage *AstralIconPackImage(NSString *name)
{
    if (![name hasPrefix:@"ic_"] || [name containsString:@"/"] || [name containsString:@".."] || !AstralSettings()) return nil;
    @synchronized(NSCache.class) {
        if (!gIcons) gIcons = [NSCache new];
        id cached = [gIcons objectForKey:name];
        if (cached) return cached == NSNull.null ? nil : cached;
        NSString *directory = ActiveDirectory();
        UIImage *image = nil;
        for (NSUInteger index = 0; directory && !image && index < sizeof(kScales) / sizeof(kScales[0]); index++) {
            NSString *file = [NSString stringWithFormat:@"%@@%lux.png", name, (unsigned long)kScales[index]];
            NSData *data =
                [NSData dataWithContentsOfFile:[[directory stringByAppendingPathComponent:@"icons"] stringByAppendingPathComponent:file]];
            if (data) image = [[UIImage imageWithData:data scale:kScales[index]] imageWithRenderingMode:UIImageRenderingModeAlwaysOriginal];
        }
        [gIcons setObject:image ?: (id)NSNull.null forKey:name];
        return image;
    }
}
