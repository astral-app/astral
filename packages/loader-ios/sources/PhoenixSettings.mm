#import "PhoenixSettings.h"

#import <UIKit/UIKit.h>
#import <mach-o/dyld.h>
#import <objc/runtime.h>
#import <pthread.h>
#import <string>
#import <vector>

#import "AstralState.h"
#import "DevServer.h"
#import "PhoenixHostCall.h"
#import "PhoenixImage.h"
#import "PhoenixLocate.h"
#import "PhoenixWire.h"

static const char kOpsArrivedNeedle[] = "ops batch dropped";

typedef struct {
    uint64_t opsArrived;
    uint64_t hostCall;
} HookPoints;

static HookPoints LocateHookPoints(const struct mach_header *image)
{
    NSString *uuid = AstralPhoenixUUID();
    NSString *cachePath = [AstralStateDirectory() stringByAppendingPathComponent:@"hooks.json"];
    NSDictionary *cache = @{};
    NSData *stored = [NSData dataWithContentsOfFile:cachePath];
    id parsed = stored ? [NSJSONSerialization JSONObjectWithData:stored options:0 error:nil] : nil;
    if ([parsed isKindOfClass:NSDictionary.class]) cache = parsed;
    NSDictionary *cached = [cache[uuid] isKindOfClass:NSDictionary.class] ? cache[uuid] : nil;
    if ([cached[@"opsArrived"] unsignedLongLongValue] && [cached[@"hostCall"] unsignedLongLongValue])
        return {[cached[@"opsArrived"] unsignedLongLongValue], [cached[@"hostCall"] unsignedLongLongValue]};
    AstralImageView view = {(const uint8_t *)image, 0, 0, 0};
    HookPoints points = {astral_locate_function_referencing(&view, kOpsArrivedNeedle), 0};
    points.hostCall = astral_locate_host_call(&view, points.opsArrived);
    if (points.opsArrived && uuid.length) {
        NSMutableDictionary *next = [cache mutableCopy];
        next[uuid] = @{@"opsArrived" : @(points.opsArrived), @"hostCall" : @(points.hostCall)};
        [[NSJSONSerialization dataWithJSONObject:next options:0 error:nil] writeToFile:cachePath atomically:YES];
    }
    return points;
}

static NSString *const kOverviewRow = @"settings.row.astral";
static NSString *const kOverviewRowPrefix = @"settings.row.astral.";
static NSString *const kBack = @"astral.back";
static NSString *const kRowPrefix = @"astral.row.";
static NSString *const kSwitchPrefix = @"astral.switch.";

static pthread_mutex_t gLock = PTHREAD_MUTEX_INITIALIZER;
static AstralHostCall gHostOriginal;
static void *gHostContext;
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

static bool InterceptHostCall(void *context, const uint8_t *frame, size_t length, void *result)
{
    if (length < 4 || frame[0] || frame[1] || frame[2] || frame[3]) return gHostOriginal(context, frame, length, result);
    pthread_mutex_lock(&gLock);
    gHostContext = context;
    bool handled;
    @autoreleasepool {
        Capture([NSData dataWithBytesNoCopy:(void *)(frame + 4) length:length - 4 freeWhenDone:NO]);
        AstralWireBuffer rewritten;
        if (astral_settings_rewrite(frame + 4, length - 4, &rewritten)) {
            std::vector<uint8_t> replacement(4 + rewritten.length, 0);
            memcpy(replacement.data() + 4, rewritten.bytes, rewritten.length);
            astral_wire_buffer_free(&rewritten);
            handled = gHostOriginal(context, replacement.data(), replacement.size(), result);
        } else {
            handled = gHostOriginal(context, frame, length, result);
        }
    }
    pthread_mutex_unlock(&gLock);
    return handled;
}

static NSString *gRoute = @"none";
static void (^gRouteChanged)(void);

static void SetRoute(NSString *route)
{
    gRoute = route;
    void (^changed)(void) = gRouteChanged;
    if (changed) dispatch_async(dispatch_get_main_queue(), changed);
}

void AstralOnPhoenixRouteChange(void (^changed)(void))
{
    gRouteChanged = [changed copy];
}

static void Log(NSString *line)
{
    NSLog(@"astral %@", line);
    AstralDevAppendLog(line);
}

NSString *AstralPhoenixRoute(void)
{
    return gRoute;
}

static void InstallHostCall(const struct mach_header *image, uint64_t hostCall)
{
    uintptr_t base = (uintptr_t)image;
    uintptr_t textSize = 0;
    const uint8_t *cursor = (const uint8_t *)((const struct mach_header_64 *)image + 1);
    for (uint32_t index = 0; index < ((const struct mach_header_64 *)image)->ncmds; index++) {
        const struct segment_command_64 *segment = (const struct segment_command_64 *)cursor;
        if (segment->cmd == LC_SEGMENT_64 && strcmp(segment->segname, "__TEXT") == 0) textSize = (uintptr_t)segment->vmsize;
        cursor += segment->cmdsize;
    }
    AstralHostCallImage target = {base, base + textSize, base + (uintptr_t)hostCall};
    gHostOriginal = (AstralHostCall)target.call;
    gRoute = @"host-call-pending";
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        for (int attempt = 0; attempt < 400; attempt++) {
            void *context = NULL;
            size_t swapped = astral_host_call_swap(&target, InterceptHostCall, &context);
            if (swapped) {
                pthread_mutex_lock(&gLock);
                if (!gHostContext) gHostContext = context;
                pthread_mutex_unlock(&gLock);
                SetRoute(@"host-call");
                Log([NSString stringWithFormat:@"phoenix: host call route installed (%zu record(s), attempt %d)", swapped, attempt + 1]);
                return;
            }
            usleep(attempt < 100 ? 20000 : 100000);
        }
        SetRoute(@"host-call-missing");
        Log(@"phoenix: host call record not found");
    });
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
    std::vector<uint8_t> frame(4 + buffer->length, 0);
    memcpy(frame.data() + 4, buffer->bytes, buffer->length);
    astral_wire_buffer_free(buffer);
    gHostOriginal(gHostContext, frame.data(), frame.size(), NULL);
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
        if (built && gHostOriginal && gHostContext)
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
    HookPoints points = LocateHookPoints(image);
    if (!points.opsArrived) return @"ops-entry-not-found";
    if (!points.hostCall) return @"host-call-not-found";
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
    InstallHostCall(image, points.hostCall);
    return nil;
}

void AstralRefreshPhoenixSettings(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (gPageModel && astral_page_depth() > 0) RefreshTop();
    });
}
