#import "PhoenixSettings.h"

#import <UIKit/UIKit.h>
#import <dlfcn.h>
#import <mach-o/dyld.h>
#import <objc/runtime.h>
#import <pthread.h>
#import <string>
#import <substrate.h>
#import <vector>

#import "AstralState.h"
#import "PhoenixImage.h"
#import "PhoenixWire.h"

static const uintptr_t kOpsArrived = 0x333ce8;
static const uint32_t kPrologue[] = {0xa9ba6ffc, 0xa90167fa, 0xa9025ff8, 0xa90357f6};
static NSString *const kOverviewRow = @"settings.row.astral";
static NSString *const kOverviewRowPrefix = @"settings.row.astral.";
static NSString *const kBack = @"astral.back";
static NSString *const kRowPrefix = @"astral.row.";
static NSString *const kSwitchPrefix = @"astral.switch.";

typedef struct {
    uint64_t first;
    uint64_t second;
} SwiftData;

typedef void *(*DataToObjC)(uint64_t, uint64_t);
typedef SwiftData (*DataFromObjC)(void *);

extern "C" {
void *astral_wire_original;
void astral_wire_trampoline(void);
void astral_wire_call(uint64_t first, uint64_t second, void *context, void *function);
void astral_wire_unlock(void);
SwiftData astral_wire_intercept(uint64_t first, uint64_t second, void *context);
}

static DataToObjC gToObjC;
static DataFromObjC gFromObjC;
static pthread_mutex_t gLock = PTHREAD_MUTEX_INITIALIZER;
static void *gContext;
static dispatch_queue_t gQueue;
static AstralSettingsPage gPageModel;
static AstralSettingsAction gAction;
static NSMutableArray<NSString *> *gRoutes;
static Class gGuestTap;
static void (*originalSetIdentifier)(UIView *, SEL, NSString *);
static BOOL (*originalShouldReceive)(id, SEL, UIGestureRecognizer *, UITouch *);
static const char gRoutedKey = 0;
static NSString *gCaptureDirectory;
static dispatch_queue_t gCaptureQueue;
static NSUInteger gCaptured;
static const NSUInteger kMaxCaptured = 400;

static void Capture(NSData *batch)
{
    if (!gCaptureDirectory || gCaptured >= kMaxCaptured) return;
    NSUInteger sequence = ++gCaptured;
    NSData *copy = [batch copy];
    dispatch_async(gCaptureQueue, ^{
        NSString *name = [NSString stringWithFormat:@"%04lu.bin", (unsigned long)sequence];
        [copy writeToFile:[gCaptureDirectory stringByAppendingPathComponent:name] atomically:NO];
    });
}

static void StartCapture(void)
{
    NSString *state = AstralStateDirectory();
    if (![NSFileManager.defaultManager fileExistsAtPath:[state stringByAppendingPathComponent:@"capture-wire"]]) return;
    NSString *directory = [state stringByAppendingPathComponent:@"wire"];
    [NSFileManager.defaultManager removeItemAtPath:directory error:nil];
    [NSFileManager.defaultManager createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:nil error:nil];
    gCaptureQueue = dispatch_queue_create("app.astral.wire-capture", DISPATCH_QUEUE_SERIAL);
    gCaptureDirectory = directory;
}

void astral_wire_unlock(void)
{
    pthread_mutex_unlock(&gLock);
}

SwiftData astral_wire_intercept(uint64_t first, uint64_t second, void *context)
{
    pthread_mutex_lock(&gLock);
    gContext = context;
    SwiftData original = {first, second};
    @autoreleasepool {
        NSData *data = CFBridgingRelease(gToObjC(first, second));
        if (![data isKindOfClass:NSData.class]) return original;
        Capture(data);
        AstralWireBuffer rewritten;
        if (!astral_settings_rewrite((const uint8_t *)data.bytes, data.length, &rewritten)) return original;
        NSData *replacement = [NSData dataWithBytesNoCopy:rewritten.bytes length:rewritten.length freeWhenDone:YES];
        return gFromObjC((__bridge void *)replacement);
    }
}

static const char *Text(NSDictionary *row, NSString *key)
{
    id value = row[key];
    return [value isKindOfClass:NSString.class] && [value length] ? [value UTF8String] : NULL;
}

static void WithSpec(NSDictionary *model, void (^body)(const AstralPageSpec *page))
{
    NSArray<NSDictionary *> *sections = model[@"sections"];
    std::vector<std::vector<AstralRowSpec>> rows(sections.count);
    std::vector<AstralSectionSpec> specs;
    for (NSUInteger index = 0; index < sections.count; index++) {
        for (NSDictionary *row in sections[index][@"rows"]) {
            rows[index].push_back({
                Text(row, @"key") ?: "",
                Text(row, @"title") ?: "",
                Text(row, @"subtitle"),
                Text(row, @"detail"),
                Text(row, @"icon"),
                (AstralRowKind)[row[@"kind"] intValue],
                [row[@"on"] boolValue],
            });
        }
        specs.push_back({Text(sections[index], @"title") ?: "", rows[index].data(), rows[index].size()});
    }
    std::vector<AstralChipSpec> chips;
    NSDictionary *toolbarModel = [model[@"toolbar"] isKindOfClass:NSDictionary.class] ? model[@"toolbar"] : nil;
    for (NSDictionary *chip in toolbarModel[@"chips"]) {
        chips.push_back({Text(chip, @"key") ?: "", Text(chip, @"title") ?: "", [chip[@"selected"] boolValue]});
    }
    AstralToolbarSpec toolbar = {chips.data(), chips.size(), Text(toolbarModel, @"buttonKey"), Text(toolbarModel, @"buttonIcon")};
    std::vector<AstralCardSpec> cards;
    for (NSDictionary *card in model[@"cards"]) {
        AstralCardSpec spec = {};
        spec.key = Text(card, @"key") ?: "";
        spec.title = Text(card, @"title") ?: "";
        spec.authors = Text(card, @"authors");
        spec.description = Text(card, @"description");
        spec.version = Text(card, @"version");
        spec.icon = Text(card, @"icon");
        spec.error = Text(card, @"error");
        for (NSString *avatar in card[@"avatars"]) {
            if (spec.avatarCount < 3 && [avatar isKindOfClass:NSString.class]) spec.avatars[spec.avatarCount++] = avatar.UTF8String;
        }
        spec.hasSettings = [card[@"hasSettings"] boolValue];
        spec.on = [card[@"on"] boolValue];
        spec.failed = [card[@"failed"] boolValue];
        cards.push_back(spec);
    }
    AstralPageSpec page = {Text(model, @"title") ?: "",    specs.data(), specs.size(),
                           toolbarModel ? &toolbar : NULL, cards.data(), cards.size()};
    body(&page);
}

typedef enum { kPush, kRefresh, kPop } Delivery;

static void Send(AstralWireBuffer *buffer)
{
    NSData *data = [NSData dataWithBytesNoCopy:buffer->bytes length:buffer->length freeWhenDone:YES];
    SwiftData swift = gFromObjC((__bridge void *)data);
    astral_wire_call(swift.first, swift.second, gContext, astral_wire_original);
}

static void Deliver(Delivery delivery, NSDictionary *model, void (^failed)(void))
{
    dispatch_async(gQueue, ^{
        pthread_mutex_lock(&gLock);
        __block AstralWireBuffer buffer = {};
        __block int built = 0;
        if (delivery == kPop) {
            built = astral_page_pop(&buffer);
        } else {
            WithSpec(model, ^(const AstralPageSpec *page) {
                built = delivery == kPush ? astral_page_push(page, &buffer) : astral_page_refresh(page, &buffer);
            });
        }
        if (built && gContext)
            Send(&buffer);
        else if (built)
            astral_wire_buffer_free(&buffer);
        pthread_mutex_unlock(&gLock);
        if (!built && failed) dispatch_async(dispatch_get_main_queue(), failed);
    });
}

static void Push(NSString *route)
{
    [gRoutes addObject:route];
    NSUInteger depth = gRoutes.count;
    Deliver(kPush, gPageModel(route), ^{
        if (gRoutes.count == depth) [gRoutes removeLastObject];
    });
}

static void RefreshTop(void)
{
    if (!gRoutes.count) return;
    Deliver(kRefresh, gPageModel(gRoutes.lastObject), nil);
}

static void Pop(void)
{
    if (!gRoutes.count) return;
    [gRoutes removeLastObject];
    Deliver(kPop, nil, nil);
    RefreshTop();
}

@interface AstralSettingsTarget : NSObject
@end

@implementation AstralSettingsTarget
- (void)tapped:(UITapGestureRecognizer *)recognizer
{
    if (recognizer.state != UIGestureRecognizerStateEnded) return;
    NSString *identifier = recognizer.view.accessibilityIdentifier;
    if ([identifier hasPrefix:kOverviewRowPrefix] || [identifier isEqualToString:kOverviewRow]) {
        if (astral_page_depth() > 0) return;
        [gRoutes removeAllObjects];
        NSString *route = [identifier hasPrefix:kOverviewRowPrefix] ? [identifier substringFromIndex:kOverviewRowPrefix.length] : @"";
        Push(route);
    } else if ([identifier isEqualToString:kBack]) {
        Pop();
    } else {
        BOOL isSwitch = [identifier hasPrefix:kSwitchPrefix];
        NSString *prefix = isSwitch ? kSwitchPrefix : kRowPrefix;
        if (![identifier hasPrefix:prefix] || !gRoutes.count) return;
        NSString *key = [identifier substringFromIndex:prefix.length];
        NSString *next = gAction(gRoutes.lastObject, isSwitch ? [@"switch." stringByAppendingString:key] : key);
        if ([next isEqualToString:@".."])
            Pop();
        else if (next.length)
            Push(next);
        else
            RefreshTop();
    }
}
@end

static AstralSettingsTarget *gTarget;

static BOOL Routed(NSString *identifier)
{
    return [identifier hasPrefix:kOverviewRow] || [identifier isEqualToString:kBack] || [identifier hasPrefix:kRowPrefix] ||
           [identifier hasPrefix:kSwitchPrefix];
}

static void Route(UIView *view)
{
    for (UIGestureRecognizer *recognizer in view.gestureRecognizers) {
        if (![recognizer isKindOfClass:gGuestTap] || objc_getAssociatedObject(recognizer, &gRoutedKey)) continue;
        [recognizer removeTarget:nil action:NULL];
        [recognizer addTarget:gTarget action:@selector(tapped:)];
        objc_setAssociatedObject(recognizer, &gRoutedKey, @YES, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
}

static void SetIdentifier(UIView *view, SEL selector, NSString *identifier)
{
    originalSetIdentifier(view, selector, identifier);
    if (!Routed(identifier)) return;
    __weak UIView *weakView = view;
    dispatch_async(dispatch_get_main_queue(), ^{
        UIView *strongView = weakView;
        if (strongView && [strongView.accessibilityIdentifier isEqualToString:identifier]) Route(strongView);
    });
}

static BOOL ShouldReceive(id navigator, SEL selector, UIGestureRecognizer *recognizer, UITouch *touch)
{
    if (astral_page_depth() > 0 && [recognizer isKindOfClass:UIPanGestureRecognizer.class]) return NO;
    return originalShouldReceive(navigator, selector, recognizer, touch);
}

static void InstallCategory(NSDictionary *category)
{
    NSArray<NSDictionary *> *entries = category[@"rows"];
    std::vector<AstralRowSpec> rows;
    for (NSDictionary *entry in entries) {
        rows.push_back({Text(entry, @"key") ?: "", Text(entry, @"title") ?: "", NULL, NULL, Text(entry, @"icon"), kAstralRowLink, 0});
    }
    astral_wire_set_category(Text(category, @"title") ?: "astral", rows.data(), rows.size());
}

NSString *AstralInstallPhoenixSettings(NSDictionary *category, AstralSettingsPage page, AstralSettingsAction action)
{
    if (gPageModel) return nil;
    const struct mach_header *image = AstralPhoenixImage();
    if (!image) return @"unsupported-image";
    const uint32_t *entry = (const uint32_t *)((const uint8_t *)image + kOpsArrived);
    if (memcmp(entry, kPrologue, sizeof(kPrologue)) != 0) return @"ops-entry-mismatch";
    gToObjC = (DataToObjC)dlsym(RTLD_DEFAULT, "$s10Foundation4DataV19_bridgeToObjectiveCSo6NSDataCyF");
    gFromObjC = (DataFromObjC)dlsym(RTLD_DEFAULT, "$s10Foundation4DataV36_unconditionallyBridgeFromObjectiveCyACSo6NSDataCSgFZ");
    if (!gToObjC || !gFromObjC) return @"data-bridge-unavailable";
    gGuestTap = NSClassFromString(@"LibdiscordHost.GuestTapRecognizer");
    if (!gGuestTap) return @"tap-recognizer-unavailable";
    Class navigator = NSClassFromString(@"LibdiscordHost.NavigatorBox");
    Method receive = navigator ? class_getInstanceMethod(navigator, @selector(gestureRecognizer:shouldReceiveTouch:)) : nullptr;
    if (!receive || strcmp(method_getTypeEncoding(receive), "B32@0:8@16@24") != 0) return @"navigator-unavailable";

    gPageModel = [page copy];
    gAction = [action copy];
    gRoutes = [NSMutableArray array];
    InstallCategory(category);
    StartCapture();
    gTarget = [AstralSettingsTarget new];
    gQueue = dispatch_queue_create("app.astral.phoenix-settings", DISPATCH_QUEUE_SERIAL);
    originalShouldReceive = (BOOL (*)(id, SEL, UIGestureRecognizer *, UITouch *))method_setImplementation(receive, (IMP)ShouldReceive);
    Method identifier = class_getInstanceMethod(UIView.class, @selector(setAccessibilityIdentifier:));
    originalSetIdentifier = (void (*)(UIView *, SEL, NSString *))method_setImplementation(identifier, (IMP)SetIdentifier);
    MSHookFunction((void *)entry, (void *)astral_wire_trampoline, &astral_wire_original);
    return astral_wire_original ? nil : @"hook-failed";
}

void AstralRefreshPhoenixSettings(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (gPageModel && astral_page_depth() > 0) RefreshTop();
    });
}
