#import "EmojiPacks.h"

#include <string>

#import "AstralState.h"
#import "TomlDocument.h"
#include "AstralSettings.h"

static NSString *const kNoEmojiPack = @"discord";
static NSString *const kZeroWidthJoiner = @"200d";
static NSString *const kVariationSelector = @"fe0f";

static NSString *gLoadedPack;
static NSDictionary *gLoadedManifest;
static NSSet<NSString *> *gCoverage;
static NSSet<NSString *> *gAnimatedCoverage;

static NSString *LocalEmojiPacksDirectory(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"emoji-packs"];
}

static NSDictionary *LoadEmojiPack(NSString *directory, BOOL bundled)
{
    NSDictionary *manifest = AstralParseTOML([NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:@"manifest.toml"]]);
    if (![manifest[@"id"] isKindOfClass:NSString.class] || ![manifest[@"name"] isKindOfClass:NSString.class] ||
        ![manifest[@"static"] isKindOfClass:NSString.class])
        return nil;
    NSMutableDictionary *pack = [manifest mutableCopy];
    pack[@"directory"] = directory;
    pack[@"bundled"] = @(bundled);
    return pack;
}

NSArray<NSDictionary *> *AstralEmojiPacks(void)
{
    NSMutableArray *packs = [NSMutableArray arrayWithObject:@{
        @"id" : kNoEmojiPack,
        @"name" : @"Discord",
        @"description" : @"Discord's own emoji",
        @"bundled" : @YES
    }];
    NSMutableSet *seen = [NSMutableSet setWithObject:kNoEmojiPack];
    NSString *bundled = [AstralResourceDirectory() stringByAppendingPathComponent:@"emoji-packs"];
    for (NSString *root in @[ LocalEmojiPacksDirectory(), bundled ]) {
        for (NSString *entry in
             [[NSFileManager.defaultManager contentsOfDirectoryAtPath:root error:nil] sortedArrayUsingSelector:@selector(compare:)]) {
            NSDictionary *pack = LoadEmojiPack([root stringByAppendingPathComponent:entry], root == bundled);
            if (!pack || [seen containsObject:pack[@"id"]]) continue;
            [seen addObject:pack[@"id"]];
            [packs addObject:pack];
        }
    }
    return packs;
}

NSString *AstralActiveEmojiPackID(void)
{
    if (!AstralSettings()) return kNoEmojiPack;
    std::string active = AstralSettings()->getString("astral", "emoji-pack", kNoEmojiPack.UTF8String);
    return [NSString stringWithUTF8String:active.c_str()] ?: kNoEmojiPack;
}

BOOL AstralEmojiAnimated(void)
{
    return AstralSettings() && AstralSettings()->getBoolean("astral", "emoji-animated", true);
}

static void Forget(void)
{
    @synchronized(NSSet.class) {
        gLoadedPack = nil;
        gLoadedManifest = nil;
        gCoverage = nil;
        gAnimatedCoverage = nil;
    }
}

void AstralSelectEmojiPack(NSString *identifier)
{
    if (AstralSettings()->set("astral", "emoji-pack", std::string(identifier.UTF8String))) AstralSettings()->save();
    Forget();
}

void AstralSetEmojiAnimated(BOOL animated)
{
    if (AstralSettings()->set("astral", "emoji-animated", (bool)animated)) AstralSettings()->save();
}

static NSSet<NSString *> *Coverage(NSString *directory, NSString *file)
{
    NSString *text = [NSString stringWithContentsOfFile:[directory stringByAppendingPathComponent:file]
                                               encoding:NSUTF8StringEncoding
                                                  error:nil];
    NSMutableSet *keys = [NSMutableSet set];
    for (NSString *line in [text componentsSeparatedByCharactersInSet:NSCharacterSet.newlineCharacterSet]) {
        if (line.length) [keys addObject:line];
    }
    return keys;
}

static BOOL LoadActive(void)
{
    NSString *active = AstralActiveEmojiPackID();
    if ([active isEqualToString:kNoEmojiPack]) return NO;
    if ([gLoadedPack isEqualToString:active]) return gLoadedManifest != nil;
    gLoadedPack = active;
    gLoadedManifest = nil;
    for (NSDictionary *pack in AstralEmojiPacks()) {
        if (![pack[@"id"] isEqualToString:active]) continue;
        gLoadedManifest = pack;
        gCoverage = Coverage(pack[@"directory"], @"coverage.txt");
        gAnimatedCoverage = Coverage(pack[@"directory"], @"animated.txt");
    }
    return gLoadedManifest != nil;
}

static NSString *CoverageKey(NSString *codepoints)
{
    NSMutableString *key = [NSMutableString string];
    for (NSString *point in [codepoints componentsSeparatedByString:@"-"]) {
        if ([point isEqualToString:kZeroWidthJoiner])
            [key appendString:@"+"];
        else if ([point isEqualToString:kVariationSelector])
            [key appendString:@"!"];
        else if (!key.length || [key hasSuffix:@"+"])
            [key appendString:point];
        else
            [key appendFormat:@"-%@", point];
    }
    return key;
}

NSString *AstralEmojiURL(NSString *codepoints)
{
    if (!codepoints.length || codepoints.length > 96 || !AstralSettings()) return nil;
    NSString *lower = codepoints.lowercaseString;
    @synchronized(NSSet.class) {
        if (!LoadActive()) return nil;
        NSString *key = CoverageKey(lower);
        if (![gCoverage containsObject:key]) return nil;
        BOOL animated =
            AstralEmojiAnimated() && [gLoadedManifest[@"animated"] isKindOfClass:NSString.class] && [gAnimatedCoverage containsObject:key];
        NSString *pattern = animated ? gLoadedManifest[@"animated"] : gLoadedManifest[@"static"];
        return [pattern stringByReplacingOccurrencesOfString:@"{codepoints}" withString:lower];
    }
}

int AstralEmojiResolve(const char *codepoints, char *url, size_t capacity)
{
    @autoreleasepool {
        NSString *resolved = AstralEmojiURL([NSString stringWithUTF8String:codepoints] ?: @"");
        if (!resolved || resolved.length + 1 > capacity) return 0;
        strlcpy(url, resolved.UTF8String, capacity);
        return 1;
    }
}
