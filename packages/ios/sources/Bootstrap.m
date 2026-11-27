#include <dlfcn.h>
#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

#import "AppExtensions.h"
#import "AstralCore.h"
#import "IncomingLinks.h"
#import "AstralDynamicCutout.h"
#import "AstralState.h"
#import "BrowserLogin.h"
#import "BuildNumber.h"
#import "CrashReports.h"
#import "EmojiComposer.h"
#import "EngineSelection.h"
#import "EnvironmentReport.h"
#import "HostRequests.h"
#import "ImageAssets.h"
#import "FontPacks.h"
#import "LuaComposer.h"
#import "MessageMenu.h"
#import "NetworkFilter.h"
#import "PhoenixAccessGate.h"
#import "PhoenixImage.h"
#import "PhoenixSettings.h"
#import "SettingsEntry.h"
#import "SettingsPages.h"
#import "VirtualCamera.h"
#import "VirtualSession.h"

static NSString *const kAstralVersion = @ASTRAL_VERSION;
static const int32_t kReadinessStart = 0;
static const int32_t kReadinessRetry = 1;
static const int64_t kReadinessRetryNs = 5 * NSEC_PER_SEC;
static const int64_t kLaunchSuccessNs = 30 * NSEC_PER_SEC;

static NSString *stateDirectory;
static NSDictionary<NSString *, id> *engineSelection;
static NSString *settingsIntegration;
static NSString *settingsRefusal;
static NSArray<NSString *> *missingExtensions;

static void Status(NSString *stage)
{
    NSDictionary *facts = @{
        @"engine" : engineSelection ?: @{},
        @"phoenixAccess" : AstralPhoenixAccessStatus(),
        @"settings" : settingsIntegration ?: @"",
        @"settingsRefusal" : settingsRefusal ?: @"",
        @"missingExtensions" : missingExtensions ?: @[],
        @"browserLogin" : AstralBrowserLoginStatus(),
        @"route" : AstralPhoenixRoute(),
        @"device" : AstralDeviceModel(),
        @"timings" : AstralPhoenixTimings(),
    };
    NSData *data = [NSJSONSerialization dataWithJSONObject:facts options:0 error:nil] ?: [NSData data];
    astral_ios_write_status(AstralTextOf(stage), AstralTextOfData(data));
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
    if (!astral_ios_claim_start()) return;
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

static void CheckReadiness(void)
{
    NSString *compatibility = AstralPhoenixCompatibility();
    AstralReadiness readiness = astral_ios_check_readiness(AstralTextOf(compatibility));
    if (readiness.action == kReadinessStart) {
        if (!InstallSettings()) {
            Status(@"settings-hook-refused");
            return;
        }
        StartPlugins();
        astral_ios_emit_lifecycle(AstralLifecycle_Foreground);
        return;
    }
    if (readiness.report) Status(compatibility);
    if (readiness.action == kReadinessRetry) {
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, kReadinessRetryNs), dispatch_get_main_queue(), ^{
            CheckReadiness();
        });
    }
}

static void Foregrounded(void)
{
    astral_ios_mark_visible();
    if (astral_ios_launch_foregrounded()) CheckReadiness();
}

static void Backgrounded(void)
{
    if (astral_ios_launch_started()) astral_ios_emit_lifecycle(AstralLifecycle_Background);
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
    AstralRestoreDiscordBuild();
    AstralInstallBrowserLogin();
    AstralRouteIncomingLinks();
    AstralInstallNetworkFilter(^BOOL {
        return astral_ios_staff_dev_tools();
    });
    AstralFixVirtualCamera();
    engineSelection = AstralForcePhoenix();
    AstralArmPhoenixAccessUnlock();
    AstralInstallImageAssets();
    AstralInstallFontPacks();
    NSURL *documents = [NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;
    if (!documents) return;
    stateDirectory = [documents.path stringByAppendingPathComponent:@"astral"];
    if (![NSFileManager.defaultManager createDirectoryAtPath:stateDirectory withIntermediateDirectories:YES attributes:nil error:nil])
        return;
    AstralSyncVirtualSession(stateDirectory);
    AstralAttachBridge();
    Prepare();
    missingExtensions = AstralMissingAppExtensions();
    AstralOnPhoenixRouteChange(^{
        NSString *stage = AstralTakeString(astral_ios_last_stage());
        Status(stage.length ? stage : @"route-changed");
    });
    AstralStartPhoenixRouteWhenLoaded(^{
        AstralPhoenixAccessHealthy();
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
