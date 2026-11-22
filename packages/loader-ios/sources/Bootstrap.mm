#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

#include <memory>

#include "LoaderExitRecord.h"
#include "LoaderFiles.h"
#include "LoaderLaunchGuard.h"
#include "LoaderLog.h"
#include "LoaderSettings.h"
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

static std::unique_ptr<loader::SettingsStore> settings;
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
        @"plugins" : PluginSummaries()
    };
    NSData *data = [NSJSONSerialization dataWithJSONObject:report options:0 error:nullptr];
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    if (!json || !loader::writeTextFileAtomically(loader::joinPath(stateDirectory, "status.json"), String(json))) {
        loader::log(loader::LogLevel::Error, "astral", "Failed to write status.json");
    }
    loader::log(loader::LogLevel::Info, "astral", String(stage));
}

static NSString *LocalPlugins(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"plugins"];
}

loader::SettingsStore *AstralSettings(void)
{
    return settings.get();
}

NSString *AstralStateDirectory(void)
{
    return [NSString stringWithUTF8String:stateDirectory.c_str()] ?: @"";
}

NSString *AstralResourceDirectory(void)
{
    static NSString *directory;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSString *jailbroken = @"/var/jb/usr/lib/astral";
        BOOL exists = [NSFileManager.defaultManager fileExistsAtPath:jailbroken];
        directory = exists ? jailbroken : [NSBundle.mainBundle.bundlePath stringByAppendingPathComponent:@"Astral.bundle"];
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
    auto previous = loader::ExitRecord(stateDirectory).take(false);
    auto verdict = loader::LaunchGuard(stateDirectory).begin(previous);
    recovering = verdict.enterRecovery;
    bool safeMode = settings->getBoolean("astral", "safe-mode", false);
    [NSFileManager.defaultManager createDirectoryAtPath:LocalPlugins() withIntermediateDirectories:YES attributes:nil error:nil];
    AstralLuaHostStart(@[ LocalPlugins(), [AstralResourceDirectory() stringByAppendingPathComponent:@"plugins"] ], settings.get(),
                       !recovering && !safeMode);
    astral_wire_set_decorator(AstralLuaDecorate);
    astral_wire_set_relabeler(AstralLuaRelabel);
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
        loader::LaunchGuard(stateDirectory).succeed();
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
    if (![NSBundle.mainBundle.bundleIdentifier isEqualToString:@"com.hammerandchisel.discord"]) return;
    engineSelection = AstralForcePhoenix();
    AstralInstallImageAssets();
    dispatch_async(dispatch_get_main_queue(), ^{
        NSURL *documents = [NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;
        if (!documents) return;
        stateDirectory = String([documents.path stringByAppendingPathComponent:@"astral"]);
        if (!loader::createDirectories(stateDirectory)) return;
        runID = NSUUID.UUID.UUIDString;
        loader::setSystemLogSink([](loader::LogLevel, const std::string &message) {
            NSString *line = [NSString stringWithUTF8String:message.c_str()] ?: @"";
            NSLog(@"astral %@", line);
            AstralDevAppendLog(line);
        });
        settings = std::make_unique<loader::SettingsStore>(loader::joinPath(stateDirectory, "settings.json"));
        settings->load();
        AstralApplyTheme(AstralActiveThemeID());
        Status(@"injected");
        [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationDidBecomeActiveNotification
                                                        object:nil
                                                         queue:NSOperationQueue.mainQueue
                                                    usingBlock:^(__unused NSNotification *notification) {
                                                        if (started) loader::ExitRecord(stateDirectory).clear();
                                                        if (readinessScheduled) return;
                                                        readinessAttempts = 0;
                                                        CheckReadiness();
                                                    }];
        [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationDidEnterBackgroundNotification
                                                        object:nil
                                                         queue:NSOperationQueue.mainQueue
                                                    usingBlock:^(__unused NSNotification *notification) {
                                                        loader::LaunchGuard(stateDirectory).succeed();
                                                        loader::ExitRecord(stateDirectory).markEnded();
                                                    }];
        if (UIApplication.sharedApplication.applicationState == UIApplicationStateActive) CheckReadiness();
    });
}
