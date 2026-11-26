#include <dlfcn.h>
#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

#import "AppExtensions.h"
#import "AstralCore.h"
#import "AstralDynamicCutout.h"
#import "AstralState.h"
#import "BrowserLogin.h"
#import "CrashReports.h"
#import "EmojiComposer.h"
#import "EngineSelection.h"
#import "EnvironmentReport.h"
#import "HostRequests.h"
#import "ImageAssets.h"
#import "LuaComposer.h"
#import "MessageMenu.h"
#import "NetworkFilter.h"
#import "PhoenixImage.h"
#import "PhoenixSettings.h"
#import "SettingsEntry.h"
#import "SettingsPages.h"
#import "VirtualCamera.h"
#import "VirtualSession.h"

static NSString *const kAstralVersion = @ASTRAL_VERSION;
static const NSUInteger kReadinessAttempts = 24;
static const int64_t kReadinessRetryNs = 5 * NSEC_PER_SEC;
static const int64_t kLaunchSuccessNs = 30 * NSEC_PER_SEC;
static const NSInteger kStatusSchema = 2;

static NSString *stateDirectory;
static NSString *runID;
static NSString *compatibility;
static NSString *lastStage;
static NSDictionary<NSString *, id> *engineSelection;
static NSString *settingsIntegration;
static NSString *settingsRefusal;
static NSArray<NSString *> *missingExtensions;
static BOOL started;
static BOOL readinessScheduled;
static NSUInteger readinessAttempts;

static NSArray *PluginSummaries(void)
{
    NSMutableArray *summaries = [NSMutableArray array];
    for (NSDictionary *plugin in AstralPlugins()) {
        [summaries addObject:@{
            @"id" : plugin[@"id"] ?: @"",
            @"enabled" : plugin[@"enabled"] ?: @NO,
            @"running" : plugin[@"running"] ?: @NO,
            @"failed" : plugin[@"failed"] ?: @NO,
            @"error" : plugin[@"error"] ?: @""
        }];
    }
    return summaries;
}

static void Status(NSString *stage)
{
    lastStage = stage;
    NSDictionary *report = @{
        @"schema" : @(kStatusSchema),
        @"runID" : runID ?: @"",
        @"pid" : @(getpid()),
        @"stage" : stage,
        @"compatibility" : compatibility ?: @"checking",
        @"recovering" : @(astral_ios_recovering()),
        @"time" : @(NSDate.date.timeIntervalSince1970),
        @"engine" : engineSelection ?: @{},
        @"settings" : settingsIntegration ?: @"",
        @"settingsRefusal" : settingsRefusal ?: @"",
        @"missingExtensions" : missingExtensions ?: @[],
        @"browserLogin" : AstralBrowserLoginStatus(),
        @"route" : AstralPhoenixRoute(),
        @"device" : AstralDeviceModel(),
        @"timings" : AstralPhoenixTimings(),
        @"plugins" : PluginSummaries()
    };
    NSData *data = [NSJSONSerialization dataWithJSONObject:report options:0 error:nil];
    if (!data || ![data writeToFile:[stateDirectory stringByAppendingPathComponent:@"status.json"] atomically:YES])
        NSLog(@"astral Failed to write status.json");
    NSLog(@"astral %@", stage);
}

NSString *AstralStateDirectory(void)
{
    return stateDirectory ?: @"";
}

static NSString *TweakResourceDirectory(void)
{
    Dl_info info;
    if (!dladdr((const void *)AstralResourceDirectory, &info) || !info.dli_fname) return nil;
    NSString *loadedFrom = [[NSString stringWithUTF8String:info.dli_fname] stringByDeletingLastPathComponent];
    return [loadedFrom stringByAppendingPathComponent:@"../../../usr/lib/astral"].stringByStandardizingPath;
}

NSString *AstralResourceDirectory(void)
{
    static NSString *directory;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSString *tweak = TweakResourceDirectory();
        BOOL exists = tweak && [NSFileManager.defaultManager fileExistsAtPath:tweak];
        directory = exists ? tweak : [NSBundle.mainBundle.bundlePath stringByAppendingPathComponent:@"astral.bundle"];
    });
    return directory;
}

NSString *AstralVersion(void)
{
    return kAstralVersion;
}

static NSString *Architecture(void)
{
#if defined(__arm64__)
    return @"arm64";
#elif defined(__x86_64__)
    return @"x86_64";
#else
    return @"unknown";
#endif
}

static NSString *DiscordVersion(void)
{
    NSDictionary *info = NSBundle.mainBundle.infoDictionary;
    return [NSString stringWithFormat:@"%@ (%@)", info[@"CFBundleShortVersionString"] ?: @"?", info[@"CFBundleVersion"] ?: @"?"];
}

static NSData *LaunchEnvironment(void)
{
    UIDevice *device = UIDevice.currentDevice;
    NSDictionary *environment = @{
        @"install_mode" : [AstralResourceDirectory().lastPathComponent isEqualToString:@"astral.bundle"] ? @"sideload" : @"package",
        @"device" : device.model ?: @"",
        @"architecture" : Architecture(),
        @"system" : [NSString stringWithFormat:@"%@ %@", device.systemName, device.systemVersion],
        @"locale" : NSLocale.preferredLanguages.firstObject ?: @"",
    };
    return [NSJSONSerialization dataWithJSONObject:environment options:0 error:nil] ?: [NSData data];
}

static void Prepare(void)
{
    NSString *support =
        [NSFileManager.defaultManager URLsForDirectory:NSApplicationSupportDirectory inDomains:NSUserDomainMask].firstObject.path ?: @"";
    NSString *languages = [NSLocale.preferredLanguages componentsJoinedByString:@"\n"];
    NSData *environment = LaunchEnvironment();
    AstralLaunch launch = {
        AstralTextOf(kAstralVersion),
        AstralTextOf(@ASTRAL_COMMIT),
        AstralTextOf(stateDirectory),
        AstralTextOf(support),
        AstralTextOf(AstralResourceDirectory()),
        AstralTextOf(DiscordVersion()),
        AstralTextOf(languages),
        AstralTextOfData(environment),
    };
    astral_ios_prepare(&launch);
}

static NSDictionary *FallbackModel(void)
{
    return AstralSettingsPageModel(@"plugins");
}

static void FallbackAction(NSString *key)
{
    AstralSettingsPageAction(@"plugins", [key hasPrefix:@"plugin."] ? [@"switch." stringByAppendingString:key] : key);
}

static BOOL InstallSettings(void)
{
    if (settingsIntegration) return YES;
    NSString *refusal = AstralInstallPhoenixSettings();
    if (!refusal) {
        settingsIntegration = @"phoenix";
        return YES;
    }
    settingsRefusal = refusal;
    if (!AstralInstallSettingsEntry(FallbackModel, FallbackAction)) return NO;
    settingsIntegration = @"uikit-fallback";
    return YES;
}

static void StartPlugins(void)
{
    if (started) return;
    started = YES;
    AstralInitializeDynamicCutout();
    astral_ios_start();
    AstralInstallMessageMenu();
    AstralInstallEmojiComposer();
    AstralInstallLuaComposer();
    AstralStartCrashReports();
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, kLaunchSuccessNs), dispatch_get_main_queue(), ^{
        astral_ios_mark_ready();
    });
    AstralPhoenixRecordMoment(@"pluginsMs");
    AstralPhoenixPluginsReady();
    Status(astral_ios_recovering() ? @"plugins-recovery" : @"ready");
}

static BOOL Retryable(NSString *state)
{
    return [state isEqualToString:@"phoenix-image-unavailable"] || [state isEqualToString:@"phoenix-root-inactive"];
}

static void CheckReadiness(void)
{
    readinessScheduled = NO;
    readinessAttempts++;
    compatibility = AstralPhoenixCompatibility();
    if ([compatibility isEqualToString:@"ready"]) {
        if (!InstallSettings()) {
            Status(@"settings-hook-refused");
            return;
        }
        StartPlugins();
        astral_ios_emit_lifecycle(AstralLifecycle_Foreground);
        return;
    }
    if (readinessAttempts == 1 || readinessAttempts == kReadinessAttempts) Status(compatibility);
    if (readinessAttempts < kReadinessAttempts && Retryable(compatibility)) {
        readinessScheduled = YES;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, kReadinessRetryNs), dispatch_get_main_queue(), ^{
            CheckReadiness();
        });
    }
}

static void Foregrounded(void)
{
    astral_ios_mark_visible();
    if (readinessScheduled) return;
    readinessAttempts = 0;
    CheckReadiness();
}

static void Backgrounded(void)
{
    if (started) astral_ios_emit_lifecycle(AstralLifecycle_Background);
    astral_ios_mark_hidden();
}

static void ObserveLifecycle(void)
{
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    [center addObserverForName:UIApplicationDidBecomeActiveNotification
                        object:nil
                         queue:NSOperationQueue.mainQueue
                    usingBlock:^(__unused NSNotification *notification) {
                        Foregrounded();
                    }];
    [center addObserverForName:UIApplicationDidEnterBackgroundNotification
                        object:nil
                         queue:NSOperationQueue.mainQueue
                    usingBlock:^(__unused NSNotification *notification) {
                        Backgrounded();
                    }];
}

__attribute__((constructor)) static void AstralBootstrap(void)
{
    if (!AstralIsDiscordApp()) return;
    AstralInstallBrowserLogin();
    AstralInstallNetworkFilter(^BOOL {
        return astral_ios_staff_dev_tools();
    });
    AstralFixVirtualCamera();
    engineSelection = AstralForcePhoenix();
    AstralInstallImageAssets();
    NSURL *documents = [NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;
    if (!documents) return;
    stateDirectory = [documents.path stringByAppendingPathComponent:@"astral"];
    if (![NSFileManager.defaultManager createDirectoryAtPath:stateDirectory withIntermediateDirectories:YES attributes:nil error:nil])
        return;
    AstralSyncVirtualSession(stateDirectory);
    runID = NSUUID.UUID.UUIDString;
    AstralAttachBridge();
    Prepare();
    missingExtensions = AstralMissingAppExtensions();
    AstralOnPhoenixRouteChange(^{
        Status(lastStage ?: @"route-changed");
    });
    AstralStartPhoenixRouteWhenLoaded(^{
        StartPlugins();
    });
    dispatch_async(dispatch_get_main_queue(), ^{
        Status(@"injected");
        AstralStartHttpsCheck();
        AstralReportMissingAppExtensions();
        ObserveLifecycle();
        if (UIApplication.sharedApplication.applicationState == UIApplicationStateActive) CheckReadiness();
    });
}
