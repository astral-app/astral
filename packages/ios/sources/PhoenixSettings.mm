#import "PhoenixSettings.h"
#import "MessageMenu.h"

#import <UIKit/UIKit.h>
#import <dlfcn.h>
#import <mach-o/dyld.h>
#import <objc/runtime.h>
#import <pthread.h>
#import <string>
#import <atomic>
#import <sys/sysctl.h>
#import <vector>

#import "AstralState.h"
#include "AstralLuaHost.h"
#import "DevServer.h"
#import "PhoenixHostCall.h"
#import "PhoenixImage.h"
#import "PhoenixLocate.h"
#import "PhoenixProperties.h"
#import "PhoenixWire.h"
#import "StopList.h"

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
static const CGFloat kEdgeSwipeDistance = 60;
static const CGFloat kEdgeSwipeStart = 24;
static NSString *const kRowPrefix = @"astral.row.";
static NSString *const kSwitchPrefix = @"astral.switch.";
static NSString *const kSliderPrefix = @"astral.slider.";
static NSString *const kActionPrefix = @"astral.action.";
static NSString *const kTapPrefix = @"astral.tap.";

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

static void CaptureNamed(NSData *batch, NSUInteger sequence, NSString *suffix)
{
    NSData *copy = [batch copy];
    dispatch_async(gCaptureQueue, ^{
        NSString *name = [NSString stringWithFormat:@"%04lu%@", (unsigned long)sequence, suffix];
        [copy writeToFile:[gCaptureDirectory stringByAppendingPathComponent:name] atomically:NO];
    });
}

static NSUInteger Capture(NSData *batch)
{
    if (!gCaptureDirectory || gCaptured >= kMaxCaptured) return 0;
    NSUInteger sequence = ++gCaptured;
    CaptureNamed(batch, sequence, @".bin");
    return sequence;
}

static void CaptureRewrite(const uint8_t *bytes, size_t length, NSUInteger sequence)
{
    if (!gCaptureDirectory || !sequence) return;
    CaptureNamed([NSData dataWithBytes:bytes length:length], sequence, @".bin.out");
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

static NSString *gRoute = @"none";
static NSMutableDictionary<NSString *, NSNumber *> *gTimings;

static void RecordTiming(NSString *key, NSNumber *value)
{
    @synchronized(NSMutableDictionary.class) {
        if (!gTimings) gTimings = [NSMutableDictionary dictionary];
        gTimings[key] = value;
    }
}

NSDictionary<NSString *, NSNumber *> *AstralPhoenixTimings(void)
{
    @synchronized(NSMutableDictionary.class) {
        return [gTimings copy] ?: @{};
    }
}
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

static double MillisecondsSinceLaunch(void)
{
    struct kinfo_proc info;
    size_t size = sizeof info;
    int name[] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()};
    if (sysctl(name, 4, &info, &size, NULL, 0) != 0) return -1;
    struct timeval now;
    gettimeofday(&now, NULL);
    struct timeval start = info.kp_proc.p_starttime;
    return (now.tv_sec - start.tv_sec) * 1000.0 + (now.tv_usec - start.tv_usec) / 1000.0;
}

static double Rounded(double value)
{
    return round(value * 10) / 10;
}

static void RecordFirstFrame(const uint8_t *batch, size_t length)
{
    static bool recorded;
    if (recorded) return;
    recorded = true;
    int32_t identifier = 0;
    if (length >= 6) memcpy(&identifier, batch + 2, 4);
    RecordTiming(@"firstFrameMs", @(Rounded(MillisecondsSinceLaunch())));
    RecordTiming(@"firstFrameCreatesRoot", @(length >= 6 && batch[1] == 1 && identifier == 1));
    void (^changed)(void) = gRouteChanged;
    if (changed) dispatch_async(dispatch_get_main_queue(), changed);
}

static const unsigned kMaxRewriteFailures = 3;
static std::atomic<bool> gRewriteDisabled{false};

static NSString *HooksPath(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"hooks.json"];
}

static NSString *DisabledKey(void)
{
    return [NSString stringWithFormat:@"rewriteDisabled.%@.%s", AstralPhoenixUUID(), ASTRAL_VERSION];
}

static BOOL RewriteDisabledForBuild(void)
{
    NSData *stored = [NSData dataWithContentsOfFile:HooksPath()];
    NSDictionary *cache = stored ? [NSJSONSerialization JSONObjectWithData:stored options:0 error:nil] : nil;
    return [cache isKindOfClass:NSDictionary.class] && [cache[DisabledKey()] boolValue];
}

static void DisableRewriting(void)
{
    if (gRewriteDisabled.exchange(true)) return;
    NSData *stored = [NSData dataWithContentsOfFile:HooksPath()];
    id parsed = stored ? [NSJSONSerialization JSONObjectWithData:stored options:0 error:nil] : nil;
    NSMutableDictionary *cache = [parsed isKindOfClass:NSDictionary.class] ? [parsed mutableCopy] : [NSMutableDictionary dictionary];
    cache[DisabledKey()] = @YES;
    [[NSJSONSerialization dataWithJSONObject:cache options:0 error:nil] writeToFile:HooksPath() atomically:YES];
    RecordTiming(@"rewriteFailures", @(astral_wire_rewrite_failures()));
    SetRoute(@"rewrite-disabled");
    Log(@"phoenix: rewritten batches failed to parse; rewriting is off for this Phoenix build until astral updates");
}

static void Send(AstralWireBuffer *buffer);
static std::atomic<bool> gRetryScheduled;
static std::atomic<bool> gTweenScheduled;
static const int64_t kTweenFrameNs = NSEC_PER_SEC / 60;

static double MonotonicMs(void)
{
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return now.tv_sec * 1000.0 + now.tv_nsec / 1e6;
}

static void ScheduleTween(void)
{
    if (!gQueue || !astral_wire_tweens_active() || gTweenScheduled.exchange(true)) return;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, kTweenFrameNs), gQueue, ^{
        pthread_mutex_lock(&gLock);
        AstralWireBuffer buffer = {};
        bool live = gHostOriginal && gHostContext && !astral_wire_unsupported_version() && !gRewriteDisabled.load();
        if (astral_wire_tween_frame(&buffer, MonotonicMs())) {
            if (live)
                Send(&buffer);
            else
                astral_wire_buffer_free(&buffer);
        }
        pthread_mutex_unlock(&gLock);
        gTweenScheduled = false;
        ScheduleTween();
    });
}

static void ScheduleRetry(void)
{
    ScheduleTween();
    if (astral_wire_take_refresh_busy())
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC / 10), dispatch_get_main_queue(), ^{
            AstralPhoenixRefreshDecorations();
        });
    if (!gQueue || !(astral_wire_pending_mentions() || astral_wire_pending_reactions()) || gRetryScheduled.exchange(true)) return;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), gQueue, ^{
        pthread_mutex_lock(&gLock);
        AstralWireBuffer buffer = {};
        bool live = gHostOriginal && gHostContext && !astral_wire_unsupported_version() && !gRewriteDisabled.load();
        if (live && astral_wire_retry_mentions(&buffer)) Send(&buffer);
        AstralWireBuffer reactions = {};
        if (live && astral_wire_retry_reactions(&reactions)) Send(&reactions);
        pthread_mutex_unlock(&gLock);
        gRetryScheduled = false;
        ScheduleRetry();
    });
}

static const int64_t kPluginWaitNs = 300 * NSEC_PER_MSEC;
static std::atomic<bool> gPluginWaitOver{false};

static dispatch_semaphore_t PluginsReady(void)
{
    static dispatch_semaphore_t ready;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        ready = dispatch_semaphore_create(0);
    });
    return ready;
}

static void WaitForPlugins(void)
{
    if (gPluginWaitOver.load() || NSThread.isMainThread) return;
    if (dispatch_semaphore_wait(PluginsReady(), dispatch_time(DISPATCH_TIME_NOW, kPluginWaitNs)) == 0)
        dispatch_semaphore_signal(PluginsReady());
    gPluginWaitOver = true;
}

static bool InterceptHostCall(void *context, const uint8_t *frame, size_t length, void *result)
{
    WaitForPlugins();
    if (length >= 16 && frame[0] == 1 && !gRewriteDisabled.load()) {
        AstralWireBuffer call;
        if (astral_host_call_rewrite(frame, length, &call)) {
            bool answered = gHostOriginal(context, call.bytes, call.length, result);
            astral_wire_buffer_free(&call);
            return answered;
        }
    }
    if (length < 4 || frame[0] || frame[1] || frame[2] || frame[3] || gRewriteDisabled.load())
        return gHostOriginal(context, frame, length, result);
    pthread_mutex_lock(&gLock);
    gHostContext = context;
    RecordFirstFrame(frame + 4, length - 4);
    bool handled;
    @autoreleasepool {
        NSUInteger captured = Capture([NSData dataWithBytesNoCopy:(void *)(frame + 4) length:length - 4 freeWhenDone:NO]);
        int unsupportedBefore = astral_wire_unsupported_version();
        AstralWireBuffer rewritten;
        if (astral_settings_rewrite(frame + 4, length - 4, &rewritten)) {
            CaptureRewrite(rewritten.bytes, rewritten.length, captured);
            std::vector<uint8_t> replacement(4 + rewritten.length, 0);
            memcpy(replacement.data() + 4, rewritten.bytes, rewritten.length);
            astral_wire_buffer_free(&rewritten);
            handled = gHostOriginal(context, replacement.data(), replacement.size(), result);
        } else {
            handled = gHostOriginal(context, frame, length, result);
        }
        if (astral_wire_rewrite_failures() >= kMaxRewriteFailures) DisableRewriting();
        int unsupported = astral_wire_unsupported_version();
        if (unsupported && !unsupportedBefore) SetRoute([NSString stringWithFormat:@"unsupported-wire-v%d", unsupported]);
    }
    pthread_mutex_unlock(&gLock);
    ScheduleRetry();
    return handled;
}

static NSString *ResolveProperties(const struct mach_header *image)
{
    static const char *names[2048];
    static char storage[65536];
    AstralImageView view = {(const uint8_t *)image, 0, 0, 0};
    CFAbsoluteTime started = CFAbsoluteTimeGetCurrent();
    size_t count = astral_read_property_names(&view, names, 2048, storage, sizeof storage);
    if (!count) return @"property-table-not-found";
    const char *missing = NULL;
    astral_wire_resolve_properties(names, count, &missing);
    if (missing) return [NSString stringWithFormat:@"property-missing:%s", missing];
    RecordTiming(@"propertiesMs", @(round((CFAbsoluteTimeGetCurrent() - started) * 10000) / 10));
    Log([NSString
        stringWithFormat:@"phoenix: %zu view properties resolved in %.1f ms", count, (CFAbsoluteTimeGetCurrent() - started) * 1000]);
    return nil;
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
    RecordTiming(@"routeStartMs", @(Rounded(MillisecondsSinceLaunch())));
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        for (int attempt = 0; attempt < 550; attempt++) {
            void *context = NULL;
            CFAbsoluteTime scanStarted = CFAbsoluteTimeGetCurrent();
            bool fullScan = attempt % 10 == 9;
            size_t swapped = astral_host_call_swap(&target, InterceptHostCall, &context, fullScan);
            RecordTiming(@"lastScanMs", @(Rounded((CFAbsoluteTimeGetCurrent() - scanStarted) * 1000)));
            if (swapped) {
                RecordTiming(@"swapMs", @(Rounded(MillisecondsSinceLaunch())));
                RecordTiming(@"swapAttempt", @(attempt + 1));
                RecordTiming(@"recordRegionTag", @(astral_host_call_last_tag()));
                pthread_mutex_lock(&gLock);
                if (!gHostContext) gHostContext = context;
                pthread_mutex_unlock(&gLock);
                SetRoute(@"host-call");
                Log([NSString stringWithFormat:@"phoenix: host call route installed (%zu record(s), attempt %d)", swapped, attempt + 1]);
                return;
            }
            usleep(attempt < 250 ? 2000 : 100000);
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
                [row[@"steps"] intValue],
                [row[@"position"] intValue],
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
        int unsupported = astral_wire_unsupported_version();
        if (unsupported && ![gRoute hasPrefix:@"unsupported-wire"])
            SetRoute([NSString stringWithFormat:@"unsupported-wire-v%d", unsupported]);
        if (built && gHostOriginal && gHostContext && !unsupported) {
            if (delivery != kPop) astral_wire_theme_page(&buffer);
            Send(&buffer);
        } else if (built)
            astral_wire_buffer_free(&buffer);
        pthread_mutex_unlock(&gLock);
        if (!built && failed) dispatch_async(dispatch_get_main_queue(), failed);
    });
}

void AstralPhoenixUpdateTheme(void (^apply)(void))
{
    pthread_mutex_lock(&gLock);
    apply();
    pthread_mutex_unlock(&gLock);
    if (!gQueue) return;
    dispatch_async(gQueue, ^{
        pthread_mutex_lock(&gLock);
        AstralWireBuffer buffer = {};
        bool live = gHostOriginal && gHostContext && !astral_wire_unsupported_version() && !gRewriteDisabled.load();
        int updates = live ? astral_wire_recolor(&buffer) : 0;
        RecordTiming(@"recolorUpdates", @(updates));
        RecordTiming(@"recolorDropped", @(astral_wire_colors_dropped()));
        if (updates) Send(&buffer);
        pthread_mutex_unlock(&gLock);
        void (^changed)(void) = gRouteChanged;
        if (changed) dispatch_async(dispatch_get_main_queue(), changed);
    });
}

int AstralPhoenixRefreshImages(void)
{
    static unsigned generation;
    __block int updates = 0;
    if (!gQueue) return 0;
    dispatch_sync(gQueue, ^{
        pthread_mutex_lock(&gLock);
        AstralWireBuffer buffer = {};
        bool live = gHostOriginal && gHostContext && !astral_wire_unsupported_version() && !gRewriteDisabled.load();
        updates = live ? astral_wire_refresh_images(&buffer, ++generation) : 0;
        if (updates) Send(&buffer);
        pthread_mutex_unlock(&gLock);
    });
    return updates;
}

void AstralPhoenixRefreshDecorations(void)
{
    if (!gQueue) return;
    dispatch_async(gQueue, ^{
        pthread_mutex_lock(&gLock);
        AstralWireBuffer buffer = {};
        bool live = gHostOriginal && gHostContext && !astral_wire_unsupported_version() && !gRewriteDisabled.load();
        if (live && astral_wire_refresh_reactions(&buffer)) Send(&buffer);
        AstralWireBuffer texts = {};
        if (live && astral_wire_refresh_message_texts(&texts)) Send(&texts);
        AstralWireBuffer tags = {};
        if (live && astral_wire_refresh_author_tags(&tags)) Send(&tags);
        AstralWireBuffer backgrounds = {};
        if (live && astral_wire_refresh_message_backgrounds(&backgrounds)) Send(&backgrounds);
        AstralWireBuffer patches = {};
        if (live && astral_wire_refresh_node_patches(&patches)) Send(&patches);
        AstralWireBuffer inserts = {};
        if (live && astral_wire_refresh_node_inserts(&inserts)) Send(&inserts);
        AstralWireBuffer commands = {};
        if (live && astral_wire_take_commands(&commands)) Send(&commands);
        bool busy = astral_wire_take_refresh_busy();
        pthread_mutex_unlock(&gLock);
        ScheduleRetry();
        if (busy)
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC / 10), dispatch_get_main_queue(), ^{
                AstralPhoenixRefreshDecorations();
            });
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

@interface AstralSettingsTarget : NSObject <UIGestureRecognizerDelegate>
@end

@implementation AstralSettingsTarget
- (void)edgeSwiped:(UIPanGestureRecognizer *)recognizer
{
    if (recognizer.state != UIGestureRecognizerStateEnded || astral_page_depth() == 0) return;
    if ([recognizer translationInView:recognizer.view].x >= kEdgeSwipeDistance) Pop();
}

- (BOOL)gestureRecognizerShouldBegin:(UIGestureRecognizer *)recognizer
{
    if (astral_page_depth() == 0 || ![recognizer isKindOfClass:UIPanGestureRecognizer.class]) return NO;
    UIPanGestureRecognizer *pan = (UIPanGestureRecognizer *)recognizer;
    CGPoint touch = [pan locationInView:pan.view];
    CGPoint moved = [pan translationInView:pan.view];
    return touch.x - moved.x <= kEdgeSwipeStart && moved.x > fabs(moved.y);
}

- (BOOL)gestureRecognizer:(UIGestureRecognizer *)recognizer shouldRecognizeSimultaneouslyWithGestureRecognizer:(UIGestureRecognizer *)other
{
    return YES;
}

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
    } else if ([identifier hasPrefix:kTapPrefix]) {
        std::string tap = identifier.UTF8String ?: "";
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            astral::luahost::pressInsertedNode(tap);
        });
    } else if ([identifier hasPrefix:kActionPrefix]) {
        std::string action = identifier.UTF8String ?: "";
        AstralDismissMessageMenu();
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            astral::luahost::pressMessageAction(action);
        });
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

static UISlider *FindSlider(UIView *view)
{
    if ([view isKindOfClass:UISlider.class]) return (UISlider *)view;
    for (UIView *child in view.subviews) {
        UISlider *found = FindSlider(child);
        if (found) return found;
    }
    return nil;
}

static NSString *Hierarchy(UIView *view, NSUInteger depth)
{
    NSMutableString *text = [NSMutableString
        stringWithFormat:@"%@%@", [@"" stringByPaddingToLength:depth * 2 withString:@" " startingAtIndex:0], NSStringFromClass(view.class)];
    if (depth < 4) {
        for (UIView *child in view.subviews) [text appendFormat:@"\n%@", Hierarchy(child, depth + 1)];
    }
    return text;
}

@interface AstralSliderTarget : NSObject
- (void)changed:(UISlider *)slider;
@end

static const char gSliderKeyKey = 0;
static const char gSliderStepsKey = 0;

@implementation AstralSliderTarget
- (void)changed:(UISlider *)slider
{
    NSString *key = objc_getAssociatedObject(slider, &gSliderKeyKey);
    int steps = [objc_getAssociatedObject(slider, &gSliderStepsKey) intValue];
    if (!key || steps <= 0 || !gRoutes.count) return;
    float span = slider.maximumValue - slider.minimumValue;
    float fraction = span > 0 ? (slider.value - slider.minimumValue) / span : 0;
    long position = lroundf(fraction * steps);
    gAction(gRoutes.lastObject, [NSString stringWithFormat:@"%@=%ld", key, position]);
    RefreshTop();
}
@end

static AstralSliderTarget *gSliderTarget;

static void RouteSlider(UIView *view, NSString *identifier)
{
    NSString *rest = [identifier substringFromIndex:kSliderPrefix.length];
    NSRange dot = [rest rangeOfString:@"."];
    if (dot.location == NSNotFound) return;
    UISlider *slider = FindSlider(view);
    if (!slider) {
        Log([NSString stringWithFormat:@"phoenix: no UISlider under %@:\n%@", identifier, Hierarchy(view, 0)]);
        return;
    }
    if (objc_getAssociatedObject(slider, &gSliderKeyKey)) return;
    Log([NSString stringWithFormat:@"phoenix: routed %@ (%@ in %@, %.0f to %.0f)", identifier, NSStringFromClass(slider.class),
                                   NSStringFromClass(view.class), slider.minimumValue, slider.maximumValue]);
    if (!gSliderTarget) gSliderTarget = [AstralSliderTarget new];
    objc_setAssociatedObject(slider, &gSliderKeyKey, [rest substringFromIndex:dot.location + 1], OBJC_ASSOCIATION_COPY_NONATOMIC);
    objc_setAssociatedObject(slider, &gSliderStepsKey, @([[rest substringToIndex:dot.location] intValue]),
                             OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    [slider addTarget:gSliderTarget action:@selector(changed:) forControlEvents:UIControlEventTouchUpInside | UIControlEventTouchUpOutside];
}

static BOOL Routed(NSString *identifier)
{
    return [identifier hasPrefix:kOverviewRow] || [identifier isEqualToString:kBack] || [identifier hasPrefix:kRowPrefix] ||
           [identifier hasPrefix:kSwitchPrefix] || [identifier hasPrefix:kActionPrefix] || [identifier hasPrefix:kTapPrefix];
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
    if ([identifier hasPrefix:kSliderPrefix]) {
        __weak UIView *weakSlider = view;
        dispatch_async(dispatch_get_main_queue(), ^{
            UIView *strongView = weakSlider;
            if (strongView && [strongView.accessibilityIdentifier isEqualToString:identifier]) RouteSlider(strongView, identifier);
        });
        return;
    }
    if (!Routed(identifier)) return;
    __weak UIView *weakView = view;
    dispatch_async(dispatch_get_main_queue(), ^{
        UIView *strongView = weakView;
        if (strongView && [strongView.accessibilityIdentifier isEqualToString:identifier]) Route(strongView);
    });
}

static const char gEdgeSwipeKey = 0;

static void AttachEdgeSwipe(UIView *host)
{
    if (!host || objc_getAssociatedObject(host, &gEdgeSwipeKey)) return;
    UIPanGestureRecognizer *swipe = [[UIPanGestureRecognizer alloc] initWithTarget:gTarget action:@selector(edgeSwiped:)];
    swipe.delegate = gTarget;
    [host addGestureRecognizer:swipe];
    objc_setAssociatedObject(host, &gEdgeSwipeKey, swipe, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}

static BOOL ShouldReceive(id navigator, SEL selector, UIGestureRecognizer *recognizer, UITouch *touch)
{
    AttachEdgeSwipe(recognizer.view);
    if (astral_page_depth() > 0 && [recognizer isKindOfClass:UIPanGestureRecognizer.class]) return NO;
    return originalShouldReceive(navigator, selector, recognizer, touch);
}

static void InstallCategory(NSDictionary *category)
{
    NSArray<NSDictionary *> *entries = category[@"rows"];
    std::vector<AstralRowSpec> rows;
    for (NSDictionary *entry in entries) {
        rows.push_back({Text(entry, @"key") ?: "", Text(entry, @"title") ?: "", NULL, NULL, Text(entry, @"icon"), kAstralRowLink, 0, 0, 0});
    }
    astral_wire_set_category(Text(category, @"title") ?: "astral", rows.data(), rows.size());
}

static NSString *gRouteRefusal;

static NSString *StartRoute(void)
{
    const struct mach_header *image = AstralPhoenixImage();
    if (!image) return @"unsupported-image";
    NSString *stopped = AstralStopListReason(AstralPhoenixUUID());
    if (stopped) {
        SetRoute(@"stopped-by-catalog");
        return [@"stopped-by-catalog: " stringByAppendingString:stopped];
    }
    HookPoints points = LocateHookPoints(image);
    if (!points.opsArrived) return @"ops-entry-not-found";
    if (!points.hostCall) return @"host-call-not-found";
    if (RewriteDisabledForBuild()) {
        SetRoute(@"rewrite-disabled");
        return @"rewrite-disabled";
    }
    NSString *propertyRefusal = ResolveProperties(image);
    if (propertyRefusal) return propertyRefusal;
    StartCapture();
    InstallHostCall(image, points.hostCall);
    return nil;
}

NSString *AstralStartPhoenixRoute(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gRouteRefusal = StartRoute();
    });
    return gRouteRefusal;
}

static void (^gRouteStarted)(void);

static void PhoenixImageAdded(const struct mach_header *header, intptr_t slide)
{
    (void)slide;
    Dl_info info;
    if (!dladdr(header, &info) || !info.dli_fname || !strstr(info.dli_fname, "/PhoenixFatClient.framework/PhoenixFatClient")) return;
    RecordTiming(@"phoenixLoadedMs", @(Rounded(MillisecondsSinceLaunch())));
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INTERACTIVE, 0), ^{
        if (!AstralStartPhoenixRoute() && gRouteStarted) dispatch_async(dispatch_get_main_queue(), gRouteStarted);
    });
}

void AstralStartPhoenixRouteWhenLoaded(void (^started)(void))
{
    gRouteStarted = [started copy];
    _dyld_register_func_for_add_image(PhoenixImageAdded);
}

NSString *AstralInstallPhoenixSettings(NSDictionary *category, AstralSettingsPage page, AstralSettingsAction action)
{
    if (gPageModel) return nil;
    NSString *routeRefusal = AstralStartPhoenixRoute();
    if (routeRefusal) return routeRefusal;
    gGuestTap = NSClassFromString(@"LibdiscordHost.GuestTapRecognizer");
    if (!gGuestTap) return @"tap-recognizer-unavailable";
    Class navigator = NSClassFromString(@"LibdiscordHost.NavigatorBox");
    Method receive = navigator ? class_getInstanceMethod(navigator, @selector(gestureRecognizer:shouldReceiveTouch:)) : nullptr;
    if (!receive || strcmp(method_getTypeEncoding(receive), "B32@0:8@16@24") != 0) return @"navigator-unavailable";

    gPageModel = [page copy];
    gAction = [action copy];
    gRoutes = [NSMutableArray array];
    pthread_mutex_lock(&gLock);
    InstallCategory(category);
    pthread_mutex_unlock(&gLock);
    gTarget = [AstralSettingsTarget new];
    gQueue = dispatch_queue_create("app.astral.phoenix-settings", DISPATCH_QUEUE_SERIAL);
    originalShouldReceive = (BOOL (*)(id, SEL, UIGestureRecognizer *, UITouch *))method_setImplementation(receive, (IMP)ShouldReceive);
    Method identifier = class_getInstanceMethod(UIView.class, @selector(setAccessibilityIdentifier:));
    originalSetIdentifier = (void (*)(UIView *, SEL, NSString *))method_setImplementation(identifier, (IMP)SetIdentifier);
    return nil;
}

void AstralRefreshPhoenixSettings(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (gPageModel && astral_page_depth() > 0) RefreshTop();
    });
}

void AstralPhoenixPluginsReady(void)
{
    dispatch_semaphore_signal(PluginsReady());
}

void AstralPhoenixRecordMoment(NSString *key)
{
    RecordTiming(key, @(Rounded(MillisecondsSinceLaunch())));
}
