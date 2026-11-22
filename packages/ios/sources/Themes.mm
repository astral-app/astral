#import "Themes.h"

#include <set>

#include <map>
#import "PhoenixSettings.h"

#import "TomlDocument.h"

#include <vector>

#include "AstralSettings.h"
#import "AstralState.h"
#import "PhoenixWire.h"

struct SurfaceTintSpec {
    AstralSurface surface;
    uint32_t color;
    double saturation;
};

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

static void AddBaseThemeEquivalents(std::vector<uint32_t> &from, std::vector<uint32_t> &to);

static NSDictionary *LoadTheme(NSString *path, BOOL bundled)
{
    NSData *data = [NSData dataWithContentsOfFile:path];
    NSDictionary *theme = data ? AstralParseTOML(data) : nil;
    if (![theme isKindOfClass:NSDictionary.class] || ![theme[@"id"] isKindOfClass:NSString.class] ||
        ![theme[@"name"] isKindOfClass:NSString.class] || ![theme[@"colors"] isKindOfClass:NSDictionary.class])
        return nil;
    NSMutableDictionary *loaded = [theme mutableCopy];
    loaded[@"bundled"] = @(bundled);
    loaded[@"path"] = path;
    return loaded;
}

NSArray<NSDictionary *> *AstralThemes(void)
{
    NSMutableArray *themes = [NSMutableArray arrayWithObject:@{
        @"id" : kDefaultTheme,
        @"name" : @"Discord",
        @"description" : @"Discord's own colours",
        @"colors" : @{},
        @"bundled" : @YES
    }];
    NSMutableSet *seen = [NSMutableSet setWithObject:kDefaultTheme];
    NSString *bundled = [AstralResourceDirectory() stringByAppendingPathComponent:@"themes"];
    for (NSString *root in @[ AstralLocalThemesDirectory(), bundled ]) {
        for (NSString *entry in
             [[NSFileManager.defaultManager contentsOfDirectoryAtPath:root error:nil] sortedArrayUsingSelector:@selector(compare:)]) {
            if (![entry.pathExtension isEqualToString:@"toml"]) continue;
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
    AddBaseThemeEquivalents(from, to);
    NSDictionary *radius = [selected[@"radius"] isKindOfClass:NSDictionary.class] ? selected[@"radius"] : nil;
    NSDictionary *font = [selected[@"font"] isKindOfClass:NSDictionary.class] ? selected[@"font"] : nil;
    NSDictionary *background = [selected[@"background"] isKindOfClass:NSDictionary.class] ? selected[@"background"] : nil;
    std::vector<uint32_t> stops;
    for (id stop in [background[@"gradient"] isKindOfClass:NSArray.class] ? background[@"gradient"] : @[]) {
        uint32_t color = [stop isKindOfClass:NSString.class] ? ParseColor(stop) : 0;
        if (color) stops.push_back(color);
    }
    double radiusScale = [radius[@"scale"] doubleValue], fontScale = [font[@"scale"] doubleValue];
    NSDictionary *tint = [selected[@"tint"] isKindOfClass:NSDictionary.class] ? selected[@"tint"] : nil;
    uint32_t tintColor = [tint[@"color"] isKindOfClass:NSString.class] ? ParseColor(tint[@"color"]) : 0;
    double tintSaturation = [tint[@"saturation"] doubleValue], tintNeutral = [tint[@"neutral"] doubleValue];
    std::vector<SurfaceTintSpec> surfaces;
    NSDictionary<NSString *, NSNumber *> *surfaceNames =
        @{@"backgrounds" : @(kAstralSurfaceBackgrounds),
          @"text" : @(kAstralSurfaceText),
          @"accents" : @(kAstralSurfaceAccents)};
    for (NSString *name in surfaceNames) {
        NSDictionary *entry = [tint[name] isKindOfClass:NSDictionary.class] ? tint[name] : nil;
        uint32_t color = [entry[@"color"] isKindOfClass:NSString.class] ? ParseColor(entry[@"color"]) : 0;
        if (color) surfaces.push_back({(AstralSurface)surfaceNames[name].intValue, color, [entry[@"saturation"] doubleValue]});
    }
    AstralPhoenixUpdateTheme(^{
        astral_wire_set_palette(from.data(), to.data(), from.size());
        astral_wire_set_scales(radiusScale, fontScale);
        astral_wire_set_background_gradient(stops.data(), stops.size());
        astral_wire_set_tint(tintColor, tintSaturation, tintNeutral);
        for (auto &surface : surfaces) astral_wire_set_surface_tint(surface.surface, surface.color, surface.saturation);
    });
    if (AstralSettings()->set("astral", "theme", std::string([selected[@"id"] UTF8String]))) AstralSettings()->save();
}

static NSDictionary<NSString *, NSString *> *DiscordPalette(void)
{
    NSString *path = [AstralResourceDirectory() stringByAppendingPathComponent:@"palettes/discord-raw.toml"];
    NSDictionary *palette = AstralParseTOML([NSData dataWithContentsOfFile:path]);
    return [palette[@"colors"] isKindOfClass:NSDictionary.class] ? palette[@"colors"] : @{};
}

static NSDictionary<NSString *, NSDictionary *> *DiscordSemanticColors(void)
{
    NSString *path = [AstralResourceDirectory() stringByAppendingPathComponent:@"palettes/discord-semantic.toml"];
    NSDictionary *palette = AstralParseTOML([NSData dataWithContentsOfFile:path]);
    return [palette[@"colors"] isKindOfClass:NSDictionary.class] ? palette[@"colors"] : @{};
}

static NSString *OpaqueHex(NSString *hex)
{
    NSString *digits = [[hex stringByReplacingOccurrencesOfString:@"#" withString:@""] uppercaseString];
    if (digits.length == 6) return [@"FF" stringByAppendingString:digits];
    if (digits.length == 8) return [[digits substringFromIndex:6] stringByAppendingString:[digits substringToIndex:6]];
    return nil;
}

static NSString *WithOpacity(NSString *argb, double opacity)
{
    if (argb.length != 8 || opacity >= 1) return argb;
    unsigned alpha = 0;
    [[NSScanner scannerWithString:[argb substringToIndex:2]] scanHexInt:&alpha];
    return [NSString stringWithFormat:@"%02X%@", (unsigned)lround(alpha * MAX(opacity, 0.0)), [argb substringFromIndex:2]];
}

static NSString *SemanticValue(NSDictionary *entry, NSString *theme, NSDictionary *palette)
{
    NSArray *value = [entry isKindOfClass:NSDictionary.class] ? entry[theme] : nil;
    if (![value isKindOfClass:NSArray.class] || value.count != 2 || ![value[0] isKindOfClass:NSString.class]) return nil;
    NSString *base = OpaqueHex(palette[value[0]] ?: @"");
    return base ? WithOpacity(base, [value[1] doubleValue]) : nil;
}

static NSString *SemanticOriginal(NSDictionary *entry, NSDictionary *palette)
{
    return SemanticValue(entry, @"dark", palette);
}

static void AddBaseThemeEquivalents(std::vector<uint32_t> &from, std::vector<uint32_t> &to)
{
    NSDictionary *raw = DiscordPalette();
    NSDictionary *semantic = DiscordSemanticColors();
    std::map<uint32_t, uint32_t> mapped;
    for (size_t index = 0; index < from.size(); index++) mapped.emplace(from[index], to[index]);
    std::set<uint32_t> known(from.begin(), from.end());
    for (NSString *token in semantic) {
        NSDictionary *entry = semantic[token];
        NSString *dark = SemanticValue(entry, @"dark", raw);
        auto found = dark ? mapped.find(ParseColor(dark)) : mapped.end();
        if (found == mapped.end()) continue;
        for (NSString *theme in @[ @"ash", @"onyx", @"light" ]) {
            NSString *equivalent = SemanticValue(entry, theme, raw);
            uint32_t value = equivalent ? ParseColor(equivalent) : 0;
            if (!value || known.count(value)) continue;
            known.insert(value);
            from.push_back(value);
            to.push_back(found->second);
        }
    }
}

static NSString *SemanticReplacement(id value)
{
    if ([value isKindOfClass:NSArray.class]) value = [value count] ? [value firstObject] : nil;
    return [value isKindOfClass:NSString.class] ? OpaqueHex(value) : nil;
}

static NSString *Slug(NSString *name)
{
    NSMutableString *slug = [NSMutableString string];
    for (NSUInteger index = 0; index < name.length; index++) {
        unichar character = [name.lowercaseString characterAtIndex:index];
        if ((character >= 'a' && character <= 'z') || (character >= '0' && character <= '9'))
            [slug appendFormat:@"%C", character];
        else if (slug.length && ![slug hasSuffix:@"-"])
            [slug appendString:@"-"];
    }
    while ([slug hasSuffix:@"-"]) [slug deleteCharactersInRange:NSMakeRange(slug.length - 1, 1)];
    return slug.length ? slug : @"imported";
}

static double ChromaOf(uint32_t color)
{
    unsigned red = (color >> 16) & 0xFF, green = (color >> 8) & 0xFF, blue = color & 0xFF;
    unsigned high = MAX(red, MAX(green, blue)), low = MIN(red, MIN(green, blue));
    return (high - low) / 255.0;
}

static NSString *TintHex(uint32_t color)
{
    return [NSString stringWithFormat:@"#%06X", color & 0xFFFFFF];
}

static NSString *DerivedTint(NSDictionary<NSString *, NSString *> *colors)
{
    uint32_t accent = 0, background = 0, text = 0;
    double accentChroma = 0, backgroundChroma = 0, textChroma = 0;
    for (NSString *original in colors) {
        uint32_t from = ParseColor(original), to = ParseColor(colors[original]);
        double chroma = ChromaOf(to);
        if (from == 0xFF5865F2) chroma += 1;
        switch (astral_wire_surface_of(from)) {
        case kAstralSurfaceAccents:
            if (chroma > accentChroma) accent = to, accentChroma = chroma;
            break;
        case kAstralSurfaceBackgrounds:
            if (chroma > backgroundChroma) background = to, backgroundChroma = chroma;
            break;
        default:
            if (chroma > textChroma) text = to, textChroma = chroma;
            break;
        }
    }
    if (!accent && !background && !text) return @"";
    NSMutableString *tint = [NSMutableString stringWithFormat:@"\n[tint]\ncolor = \"%@\"\nsaturation = 0.4\nneutral = 0.3\n",
                                                               TintHex(accent ?: background ?: text)];
    if (background && backgroundChroma > 0.03)
        [tint appendFormat:@"\n[tint.backgrounds]\ncolor = \"%@\"\nsaturation = 0.35\n", TintHex(background)];
    if (text && textChroma > 0.03) [tint appendFormat:@"\n[tint.text]\ncolor = \"%@\"\nsaturation = 0.4\n", TintHex(text)];
    return tint;
}

NSString *AstralImportUnboundTheme(NSData *json, NSString *fallbackName)
{
    id parsed = [NSJSONSerialization JSONObjectWithData:json options:0 error:nil];
    if (![parsed isKindOfClass:NSDictionary.class]) return @"not an Unbound theme";
    NSDictionary *raw = [parsed[@"raw"] isKindOfClass:NSDictionary.class] ? parsed[@"raw"] : parsed[@"rawColors"];
    NSDictionary *semantic = [parsed[@"semantic"] isKindOfClass:NSDictionary.class] ? parsed[@"semantic"] : parsed[@"semanticColors"];
    if (![raw isKindOfClass:NSDictionary.class]) raw = @{};
    if (![semantic isKindOfClass:NSDictionary.class]) semantic = @{};
    if (!raw.count && !semantic.count) return @"the theme has no raw or semantic colours to convert";
    NSDictionary *palette = DiscordPalette();
    NSMutableDictionary<NSString *, NSString *> *colors = [NSMutableDictionary dictionary];
    for (NSString *name in raw) {
        NSString *original = OpaqueHex(palette[name] ?: @""),
                 *replacement = [raw[name] isKindOfClass:NSString.class] ? OpaqueHex(raw[name]) : nil;
        if (original && replacement && ![original isEqualToString:replacement]) colors[original] = replacement;
    }
    NSDictionary *tokens = semantic.count ? DiscordSemanticColors() : @{};
    for (NSString *token in semantic) {
        NSString *original = SemanticOriginal(tokens[token], palette), *replacement = SemanticReplacement(semantic[token]);
        if (original && replacement && ![original isEqualToString:replacement]) colors[original] = replacement;
    }
    if (!colors.count) return @"none of the theme's colours match Discord's palette";
    NSString *name = [parsed[@"name"] isKindOfClass:NSString.class] ? parsed[@"name"] : fallbackName;
    NSString *identifier = [@"unbound-" stringByAppendingString:Slug(name)];
    NSMutableString *toml = [NSMutableString
        stringWithFormat:@"id = \"%@\"\nname = \"%@\"\nversion = \"0.0.1\"\ndescription = \"Imported from an Unbound theme\"\n\n[colors]\n",
                         identifier, [name stringByReplacingOccurrencesOfString:@"\"" withString:@"'"]];
    for (NSString *key in [colors.allKeys sortedArrayUsingSelector:@selector(compare:)])
        [toml appendFormat:@"%@ = \"%@\"\n", key, colors[key]];
    [toml appendString:DerivedTint(colors)];
    NSString *directory = AstralLocalThemesDirectory();
    [NSFileManager.defaultManager createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:nil error:nil];
    if (![toml writeToFile:[directory stringByAppendingPathComponent:[identifier stringByAppendingPathExtension:@"toml"]]
                atomically:YES
                  encoding:NSUTF8StringEncoding
                     error:nil]) {
        return @"could not save the theme";
    }
    AstralApplyTheme(identifier);
    return nil;
}
