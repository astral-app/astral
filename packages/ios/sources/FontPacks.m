#import "FontPacks.h"

#import "AstralCore.h"
#import <CoreText/CoreText.h>
#import <UIKit/UIKit.h>
#import <objc/runtime.h>

static const NSTimeInterval kReloadInterval = 1;

static UIFont *(*originalFontWithNameSize)(id, SEL, NSString *, CGFloat);
static NSDictionary<NSString *, NSString *> *gFaces;
static NSString *gPack;
static NSTimeInterval gCheckedAt;
static BOOL gReady;
static NSLock *gLock;

static NSString *RegisteredName(NSString *file)
{
    NSURL *url = [NSURL fileURLWithPath:file];
    CGDataProviderRef provider = CGDataProviderCreateWithURL((__bridge CFURLRef)url);
    if (!provider) return nil;
    CGFontRef font = CGFontCreateWithDataProvider(provider);
    CGDataProviderRelease(provider);
    if (!font) return nil;
    NSString *name = CFBridgingRelease(CGFontCopyPostScriptName(font));
    CGFontRelease(font);
    CTFontManagerRegisterFontsForURL((__bridge CFURLRef)url, kCTFontManagerScopeProcess, NULL);
    return name;
}

static void ReloadFaces(void)
{
    NSTimeInterval now = NSDate.timeIntervalSinceReferenceDate;
    if (gReady && now - gCheckedAt < kReloadInterval) return;
    gCheckedAt = now;
    id parsed = AstralTakeJSON(astral_ios_font_faces());
    NSDictionary *spec = [parsed isKindOfClass:NSDictionary.class] ? parsed : @{};
    gReady = [spec[@"ready"] boolValue];
    NSString *pack = [spec[@"id"] isKindOfClass:NSString.class] ? spec[@"id"] : @"";
    if ([pack isEqualToString:gPack ?: @""]) return;
    gPack = pack;
    NSMutableDictionary<NSString *, NSString *> *faces = [NSMutableDictionary dictionary];
    NSDictionary *listed = [spec[@"faces"] isKindOfClass:NSDictionary.class] ? spec[@"faces"] : @{};
    for (NSString *weight in listed) {
        NSDictionary *face = [listed[weight] isKindOfClass:NSDictionary.class] ? listed[weight] : nil;
        NSString *file = [face[@"file"] isKindOfClass:NSString.class] ? face[@"file"] : nil;
        NSString *name = file ? RegisteredName(file) : ([face[@"name"] isKindOfClass:NSString.class] ? face[@"name"] : nil);
        if (name.length) faces[weight] = name;
    }
    gFaces = faces;
}

static NSString *WeightOf(NSString *name)
{
    NSString *face = [[name substringFromIndex:@"ggsans-".length] stringByReplacingOccurrencesOfString:@"Italic" withString:@""];
    if ([face isEqualToString:@"ExtraBold"]) return @"extrabold";
    if ([face isEqualToString:@"Bold"]) return @"bold";
    if ([face isEqualToString:@"Semibold"]) return @"semibold";
    if ([face isEqualToString:@"Medium"]) return @"medium";
    return @"normal";
}

static NSString *ReplacementName(NSString *name)
{
    if (![name hasPrefix:@"ggsans-"]) return nil;
    [gLock lock];
    ReloadFaces();
    NSDictionary<NSString *, NSString *> *faces = gFaces;
    [gLock unlock];
    if (!faces.count) return nil;
    NSString *weight = WeightOf(name);
    return faces[weight] ?: faces[@"normal"];
}

static UIFont *FontWithNameSize(id self, SEL selector, NSString *name, CGFloat size)
{
    NSString *replacement = ReplacementName(name);
    UIFont *font = replacement ? originalFontWithNameSize(self, selector, replacement, size) : nil;
    return font ?: originalFontWithNameSize(self, selector, name, size);
}

void AstralInstallFontPacks(void)
{
    if (gLock) return;
    gLock = [NSLock new];
    Method method = class_getClassMethod(UIFont.class, @selector(fontWithName:size:));
    if (method)
        originalFontWithNameSize = (UIFont * (*)(id, SEL, NSString *, CGFloat)) method_setImplementation(method, (IMP)FontWithNameSize);
}
