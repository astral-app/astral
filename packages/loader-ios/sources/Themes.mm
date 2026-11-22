#import "Themes.h"

#include <vector>

#include "LoaderSettings.h"
#import "AstralState.h"
#import "PhoenixWire.h"

static NSString *const kDefaultTheme = @"discord";

NSString *AstralLocalThemesDirectory(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"themes"];
}

static uint32_t ParseColor(NSString *text)
{
    NSString *hex = [text hasPrefix:@"#"] ? [text substringFromIndex:1] : text;
    if (hex.length == 6) hex = [@"FF" stringByAppendingString:hex];
    if (hex.length != 8) return 0;
    unsigned int value = 0;
    return [[NSScanner scannerWithString:hex] scanHexInt:&value] ? value : 0;
}

static NSDictionary *LoadTheme(NSString *path, BOOL bundled)
{
    NSData *data = [NSData dataWithContentsOfFile:path];
    NSDictionary *theme = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    if (![theme isKindOfClass:NSDictionary.class] || ![theme[@"id"] isKindOfClass:NSString.class] ||
        ![theme[@"name"] isKindOfClass:NSString.class] || ![theme[@"colors"] isKindOfClass:NSDictionary.class]) return nil;
    NSMutableDictionary *loaded = [theme mutableCopy];
    loaded[@"bundled"] = @(bundled);
    loaded[@"path"] = path;
    return loaded;
}

NSArray<NSDictionary *> *AstralThemes(void)
{
    NSMutableArray *themes = [NSMutableArray arrayWithObject:@{
        @"id": kDefaultTheme, @"name": @"Discord", @"description": @"Discord's own colours", @"colors": @{}, @"bundled": @YES}];
    NSMutableSet *seen = [NSMutableSet setWithObject:kDefaultTheme];
    NSString *bundled = [AstralResourceDirectory() stringByAppendingPathComponent:@"themes"];
    for (NSString *root in @[AstralLocalThemesDirectory(), bundled]) {
        for (NSString *entry in [[NSFileManager.defaultManager contentsOfDirectoryAtPath:root error:nil] sortedArrayUsingSelector:@selector(compare:)]) {
            if (![entry.pathExtension isEqualToString:@"json"]) continue;
            NSDictionary *theme = LoadTheme([root stringByAppendingPathComponent:entry], root == bundled);
            if (!theme || [seen containsObject:theme[@"id"]]) continue;
            [seen addObject:theme[@"id"]];
            [themes addObject:theme];
        }
    }
    return themes;
}

NSString *AstralActiveThemeID(void)
{
    std::string active = AstralSettings()->getString("astral", "theme", kDefaultTheme.UTF8String);
    return [NSString stringWithUTF8String:active.c_str()] ?: kDefaultTheme;
}

void AstralApplyTheme(NSString *themeID)
{
    NSDictionary *selected = nil;
    for (NSDictionary *theme in AstralThemes()) {
        if ([theme[@"id"] isEqualToString:themeID]) selected = theme;
    }
    if (!selected) selected = AstralThemes().firstObject;
    std::vector<uint32_t> from, to;
    NSDictionary *colors = selected[@"colors"];
    for (NSString *key in colors) {
        if (![key isKindOfClass:NSString.class] || ![colors[key] isKindOfClass:NSString.class]) continue;
        uint32_t source = ParseColor(key), target = ParseColor(colors[key]);
        if (!source || !target) continue;
        from.push_back(source);
        to.push_back(target);
    }
    astral_wire_set_palette(from.data(), to.data(), from.size());
    if (AstralSettings()->set("astral", "theme", std::string([selected[@"id"] UTF8String]))) AstralSettings()->save();
}
