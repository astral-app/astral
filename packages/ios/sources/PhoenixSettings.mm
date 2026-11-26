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

#import "AstralCore.h"
#import "AstralState.h"
#import "PhoenixHostCall.h"
#import "PhoenixImage.h"
#import "PhoenixLocate.h"
#import "PhoenixProperties.h"
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
static BOOL gInstalled;
static dispatch_queue_t gNavigatorQueue;
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

static bool Live(void);
static void Flush(void);

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
    RecordTiming(@"firstFrameCreatesRoot", @((BOOL)(length >= 6 && batch[1] == 1 && identifier == 1)));
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
    if (!gQueue) return;
    bool soon = astral_ios_decorations_pending();
    bool later = astral_wire_pending_mentions();
    if ((!soon && !later) || gRetryScheduled.exchange(true)) return;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, soon ? NSEC_PER_SEC / 10 : NSEC_PER_SEC), gQueue, ^{
        gRetryScheduled = false;
        Flush();
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

static bool Live(void)
{
    return gHostOriginal && gHostContext && !astral_wire_unsupported_version() && !gRewriteDisabled.load();
}

static void SendBytes(const uint8_t *bytes, size_t length)
{
    std::vector<uint8_t> frame(4 + length, 0);
    memcpy(frame.data() + 4, bytes, length);
    gHostOriginal(gHostContext, frame.data(), frame.size(), NULL);
}

static void Send(AstralWireBuffer *buffer)
{
    SendBytes(buffer->bytes, buffer->length);
    astral_wire_buffer_free(buffer);
}

void AstralPhoenixDeliver(const uint8_t *bytes, size_t length)
{
    if (!gQueue || !bytes || !length) return;
    NSData *batch = [NSData dataWithBytes:bytes length:length];
    dispatch_async(gQueue, ^{
        pthread_mutex_lock(&gLock);
        if (Live()) SendBytes((const uint8_t *)batch.bytes, batch.length);
        pthread_mutex_unlock(&gLock);
        void (^changed)(void) = gRouteChanged;
        if (changed) dispatch_async(dispatch_get_main_queue(), changed);
    });
}

void AstralPhoenixLockWire(void)
{
    pthread_mutex_lock(&gLock);
}

void AstralPhoenixUnlockWire(void)
{
    pthread_mutex_unlock(&gLock);
}

static void Flush(void)
{
    if (!gQueue) return;
    dispatch_async(gQueue, ^{
        if (Live()) {
            AstralOwned updates = astral_ios_pending_updates();
            pthread_mutex_lock(&gLock);
            if (updates.bytes && updates.length && Live()) SendBytes(updates.bytes, updates.length);
            pthread_mutex_unlock(&gLock);
            astral_ios_free(updates);
        }
        ScheduleRetry();
    });
}

void AstralPhoenixRefreshDecorations(void)
{
    astral_ios_refresh_decorations();
    Flush();
}

static void Navigate(void (^work)(void))
{
    dispatch_async(gNavigatorQueue, work);
}

@interface AstralSettingsTarget : NSObject <UIGestureRecognizerDelegate>
@end

@implementation AstralSettingsTarget
- (void)edgeSwiped:(UIPanGestureRecognizer *)recognizer
{
    if (recognizer.state != UIGestureRecognizerStateEnded || astral_page_depth() == 0) return;
    if ([recognizer translationInView:recognizer.view].x < kEdgeSwipeDistance) return;
    Navigate(^{
        astral_ios_navigator_back();
    });
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
    if ([identifier hasPrefix:kTapPrefix]) {
        NSString *tap = [identifier copy];
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            astral_ios_press_inserted_node(AstralTextOf(tap));
        });
    } else if ([identifier hasPrefix:kActionPrefix]) {
        NSString *action = [identifier copy];
        AstralDismissMessageMenu();
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            astral_ios_press_message_action(AstralTextOf(action));
        });
    } else if (identifier && astral_ios_navigator_handles(AstralTextOf(identifier))) {
        NSString *pressed = [identifier copy];
        Navigate(^{
            astral_ios_navigator_tap(AstralTextOf(pressed));
        });
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

@implementation AstralSliderTarget
- (void)changed:(UISlider *)slider
{
    NSString *identifier = objc_getAssociatedObject(slider, &gSliderKeyKey);
    if (!identifier) return;
    float span = slider.maximumValue - slider.minimumValue;
    double fraction = span > 0 ? (slider.value - slider.minimumValue) / span : 0;
    Navigate(^{
        astral_ios_navigator_slide(AstralTextOf(identifier), fraction);
    });
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
    objc_setAssociatedObject(slider, &gSliderKeyKey, identifier, OBJC_ASSOCIATION_COPY_NONATOMIC);
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

static NSString *gRouteRefusal;

static NSString *StartRoute(void)
{
    const struct mach_header *image = AstralPhoenixImage();
    if (!image) return @"unsupported-image";
    NSString *stopped = AstralTakeString(astral_ios_stop_list_reason(AstralTextOf(AstralPhoenixUUID())));
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

NSString *AstralInstallPhoenixSettings(void)
{
    if (gInstalled) return nil;
    NSString *routeRefusal = AstralStartPhoenixRoute();
    if (routeRefusal) return routeRefusal;
    gGuestTap = NSClassFromString(@"LibdiscordHost.GuestTapRecognizer");
    if (!gGuestTap) return @"tap-recognizer-unavailable";
    Class navigator = NSClassFromString(@"LibdiscordHost.NavigatorBox");
    Method receive = navigator ? class_getInstanceMethod(navigator, @selector(gestureRecognizer:shouldReceiveTouch:)) : nullptr;
    if (!receive || strcmp(method_getTypeEncoding(receive), "B32@0:8@16@24") != 0) return @"navigator-unavailable";

    gInstalled = YES;
    gNavigatorQueue = dispatch_queue_create("app.astral.settings-navigator", DISPATCH_QUEUE_SERIAL);
    gTarget = [AstralSettingsTarget new];
    gQueue = dispatch_queue_create("app.astral.phoenix-settings", DISPATCH_QUEUE_SERIAL);
    originalShouldReceive = (BOOL (*)(id, SEL, UIGestureRecognizer *, UITouch *))method_setImplementation(receive, (IMP)ShouldReceive);
    Method identifier = class_getInstanceMethod(UIView.class, @selector(setAccessibilityIdentifier:));
    originalSetIdentifier = (void (*)(UIView *, SEL, NSString *))method_setImplementation(identifier, (IMP)SetIdentifier);
    return nil;
}

void AstralPhoenixPluginsReady(void)
{
    dispatch_semaphore_signal(PluginsReady());
}

void AstralPhoenixRecordMoment(NSString *key)
{
    RecordTiming(key, @(Rounded(MillisecondsSinceLaunch())));
}
