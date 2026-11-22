#include <dlfcn.h>
#import "NetworkFilter.h"
#import "VirtualCamera.h"
#import "VirtualSession.h"
#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

#include <memory>

#include "AstralExitRecord.h"
#include "AstralFiles.h"
#include "AstralLaunchGuard.h"
#include "AstralLog.h"
#include "AstralSettings.h"
#import "AstralState.h"
#import "DevServer.h"
#import "CrashReports.h"
#import "AuthorAvatars.h"
#import "EngineSelection.h"
#import "ImageAssets.h"
#import "LuaHost.h"
#import "Marketplace.h"
#import "PhoenixImage.h"
#import "PhoenixSettings.h"
#import "PhoenixWire.h"
#import "SettingsEntry.h"
#import "SettingsPages.h"
#import "Themes.h"

static NSString *const kAstralVersion = @ASTRAL_VERSION;

static std::unique_ptr<astral::SettingsStore> settings;
static std::string stateDirectory;
static NSString *runID;
static NSString *compatibility;
static NSString *lastStage;
static NSDictionary<NSString *, id> *engineSelection;
static NSString *settingsIntegration;
static NSString *settingsRefusal;
static BOOL started;
static BOOL recovering;
static BOOL readinessScheduled;
static NSUInteger readinessAttempts;

static std::string String(NSString *value)
{
    return value.UTF8String ?: "";
}

static NSArray *PluginSummaries(void)
{
    NSMutableArray *summaries = [NSMutableArray array];
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        [summaries addObject:@{
            @"id" : plugin[@"id"],
            @"enabled" : plugin[@"enabled"],
            @"running" : plugin[@"running"],
            @"failed" : plugin[@"failed"],
            @"error" : plugin[@"error"]
        }];
    }
    return summaries;
}

static void Status(NSString *stage)
{
    lastStage = stage;
    NSDictionary *report = @{
        @"schema" : @2,
        @"runID" : runID ?: @"",
        @"pid" : @(getpid()),
        @"stage" : stage,
        @"compatibility" : compatibility ?: @"checking",
        @"recovering" : @(recovering),
        @"time" : @(NSDate.date.timeIntervalSince1970),
        @"engine" : engineSelection ?: @{},
        @"settings" : settingsIntegration ?: @"",
        @"settingsRefusal" : settingsRefusal ?: @"",
        @"route" : AstralPhoenixRoute(),
        @"device" : AstralDeviceModel(),
        @"timings" : AstralPhoenixTimings(),
        @"plugins" : PluginSummaries()
    };
    NSData *data = [NSJSONSerialization dataWithJSONObject:report options:0 error:nullptr];
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    if (!json || !astral::writeTextFileAtomically(astral::joinPath(stateDirectory, "status.json"), String(json))) {
        astral::log(astral::LogLevel::Error, "astral", "Failed to write status.json");
    }
    astral::log(astral::LogLevel::Info, "astral", String(stage));
}

static NSString *LocalPlugins(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"plugins"];
}

astral::SettingsStore *AstralSettings(void)
{
    return settings.get();
}

NSString *AstralStateDirectory(void)
{
    return [NSString stringWithUTF8String:stateDirectory.c_str()] ?: @"";
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

static AstralPageContext PageContext(void)
{
    NSDictionary *info = NSBundle.mainBundle.infoDictionary;
    NSString *discord =
        [NSString stringWithFormat:@"%@ · %@", info[@"CFBundleShortVersionString"] ?: @"?", info[@"CFBundleVersion"] ?: @"?"];
    return {recovering, discord, compatibility ?: @"checking"};
}

static void SettingsChanged(void)
{
    AstralRefreshPhoenixSettings();
    Status(@"settings-changed");
}

static NSDictionary *FallbackModel(void)
{
    return AstralSettingsPageModel(@"plugins", PageContext());
}

static void FallbackAction(NSString *key)
{
    AstralSettingsPageAction(@"plugins", [key hasPrefix:@"plugin."] ? [@"switch." stringByAppendingString:key] : key,
                             ^{
                             });
}

static BOOL InstallSettings(void)
{
    if (settingsIntegration) return YES;
    NSString *refusal = AstralInstallPhoenixSettings(
        AstralSettingsCategory(),
        ^NSDictionary *(NSString *route) {
            return AstralSettingsPageModel(route, PageContext());
        },
        ^NSString *(NSString *route, NSString *key) {
            NSString *next = AstralSettingsPageAction(route, key, ^{
                SettingsChanged();
            });
            if ([key hasPrefix:@"switch."] || [key isEqualToString:@"uninstall"]) Status(@"settings-changed");
            return next;
        });
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
    auto previous = astral::ExitRecord(stateDirectory).take(false);
    auto verdict = astral::LaunchGuard(stateDirectory).begin(previous);
    recovering = verdict.enterRecovery;
    bool safeMode = settings->getBoolean("astral", "safe-mode", false);
    [NSFileManager.defaultManager createDirectoryAtPath:LocalPlugins() withIntermediateDirectories:YES attributes:nil error:nil];
    AstralLuaHostStart(@[ LocalPlugins(), [AstralResourceDirectory() stringByAppendingPathComponent:@"plugins"] ], settings.get(),
                       !recovering && !safeMode);
    astral_wire_set_decorator(AstralLuaDecorate);
    astral_wire_set_relabeler(AstralLuaRelabel);
    astral_wire_set_mention_decorator(AstralLuaMention);
    astral_wire_set_message_text_colorizer(AstralLuaMessageTextColor);
    astral_wire_set_name_colorizer(AstralLuaNameColor);
    AstralCatalogRefresh(^{
        AstralRefreshPhoenixSettings();
    });
    AstralAuthorAvatarsOnChange(^{
        AstralRefreshPhoenixSettings();
    });
    AstralStartDevServer(
        ^BOOL {
            return settings->getBoolean("astral", "developer-mode", false);
        },
        ^{
            AstralRefreshPhoenixSettings();
        });
    AstralStartCrashReports(^NSArray<NSString *> * {
        NSMutableArray *running = [NSMutableArray array];
        for (NSDictionary *plugin in AstralLuaPlugins()) {
            if ([plugin[@"running"] boolValue]) [running addObject:[NSString stringWithFormat:@"%@@%@", plugin[@"id"], plugin[@"version"]]];
        }
        return running;
    });
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
        astral::LaunchGuard(stateDirectory).succeed();
    });
    Status(recovering ? @"plugins-recovery" : @"ready");
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
        return;
    }
    if (readinessAttempts == 1 || readinessAttempts == 24) Status(compatibility);
    if (readinessAttempts < 24 &&
        ([compatibility isEqualToString:@"phoenix-image-unavailable"] || [compatibility isEqualToString:@"phoenix-root-inactive"])) {
        readinessScheduled = YES;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
            CheckReadiness();
        });
    }
}

__attribute__((constructor)) static void AstralBootstrap(void)
{
    if (!AstralIsDiscordApp()) return;
    AstralInstallNetworkFilter(^BOOL {
        return settings && settings->getBoolean("astral", "staff-devtools", false);
    });
    AstralFixVirtualCamera();
    engineSelection = AstralForcePhoenix();
    AstralInstallImageAssets();
    NSURL *documents = [NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;
    if (!documents) return;
    stateDirectory = String([documents.path stringByAppendingPathComponent:@"astral"]);
    if (!astral::createDirectories(stateDirectory)) return;
    AstralSyncVirtualSession([documents.path stringByAppendingPathComponent:@"astral"]);
    runID = NSUUID.UUID.UUIDString;
    astral::setSystemLogSink([](astral::LogLevel, const std::string &message) {
        NSString *line = [NSString stringWithUTF8String:message.c_str()] ?: @"";
        NSLog(@"astral %@", line);
        AstralDevAppendLog(line);
    });
    settings = std::make_unique<astral::SettingsStore>(astral::joinPath(stateDirectory, "settings.json"));
    settings->load();
    AstralApplyTheme(AstralActiveThemeID());
    AstralOnPhoenixRouteChange(^{
        Status(lastStage ?: @"route-changed");
    });
    AstralStartPhoenixRouteWhenLoaded();
    dispatch_async(dispatch_get_main_queue(), ^{
        Status(@"injected");
        [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationDidBecomeActiveNotification
                                                        object:nil
                                                         queue:NSOperationQueue.mainQueue
                                                    usingBlock:^(__unused NSNotification *notification) {
                                                        if (started) astral::ExitRecord(stateDirectory).clear();
                                                        if (readinessScheduled) return;
                                                        readinessAttempts = 0;
                                                        CheckReadiness();
                                                    }];
        [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationDidEnterBackgroundNotification
                                                        object:nil
                                                         queue:NSOperationQueue.mainQueue
                                                    usingBlock:^(__unused NSNotification *notification) {
                                                        astral::LaunchGuard(stateDirectory).succeed();
                                                        astral::ExitRecord(stateDirectory).markEnded();
                                                    }];
        if (UIApplication.sharedApplication.applicationState == UIApplicationStateActive) CheckReadiness();
    });
}
