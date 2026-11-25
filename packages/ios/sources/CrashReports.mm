#import "CrashReports.h"

#import "EnvironmentReport.h"
#import "PhoenixImage.h"

#import <MetricKit/MetricKit.h>
#import <mach-o/dyld.h>

#include <set>
#include <string>

#include "AstralEnvironmentReport.h"
#include "AstralMetricKitCrash.h"
#import "FoundationJSON.h"
#include "AstralPrivacy.h"
#include "AstralReports.h"
#import "AstralState.h"
#import "PhoenixImage.h"

static NSArray<NSString *> * (^gRunningPlugins)(void);

@interface AstralCrashSubscriber : NSObject <MXMetricManagerSubscriber>
@end

static NSDictionary *Environment(void)
{
    NSDictionary *info = NSBundle.mainBundle.infoDictionary;
#if defined(__arm64__)
    NSString *architecture = @"arm64";
#elif defined(__x86_64__)
    NSString *architecture = @"x86_64";
#else
    NSString *architecture = @"unknown";
#endif
    NSString *resourceName = AstralResourceDirectory().lastPathComponent;
    NSMutableArray<NSString *> *plugins = [NSMutableArray array];
    for (id plugin in (gRunningPlugins ? gRunningPlugins() : @[])) {
        if ([plugin isKindOfClass:NSString.class]) [plugins addObject:plugin];
    }
    return @{
        @"schema" : @2,
        @"platform" : @"ios",
        @"install_mode" : [resourceName isEqualToString:@"astral.bundle"] ? @"sideload" : @"package",
        @"astral" : AstralVersion(),
        @"discord" : [NSString stringWithFormat:@"%@ (%@)", info[@"CFBundleShortVersionString"] ?: @"?", info[@"CFBundleVersion"] ?: @"?"],
        @"engine" : @"Phoenix",
        @"phoenix" : AstralPhoenixUUID() ?: @"",
        @"device" : UIDevice.currentDevice.model ?: @"",
        @"architecture" : architecture,
        @"system" : [NSString stringWithFormat:@"%@ %@", UIDevice.currentDevice.systemName, UIDevice.currentDevice.systemVersion],
        @"locale" : NSLocale.preferredLanguages.firstObject ?: @"",
        @"plugins" : plugins,
    };
}

NSDictionary *AstralDiagnosticEnvironment(void)
{
    NSMutableDictionary *environment = [Environment() mutableCopy];
    [environment addEntriesFromDictionary:AstralEnvironmentReport()];
    environment[@"commit"] = @ASTRAL_COMMIT;
    environment[@"compatibility"] = AstralPhoenixCompatibility() ?: @"";
    auto summary = astral::environment_report::crashSummary(AstralJSONFromObject(AstralCrashReports()), NSDate.date.timeIntervalSince1970);
    [environment addEntriesFromDictionary:AstralObjectFromJSON(summary)];
    return environment;
}

void AstralRecordPluginFailure(NSString *pluginID, NSString *pluginName, NSString *error)
{
    astral::reports::pluginFailure(pluginID.UTF8String ?: "", pluginName.UTF8String ?: "Plugin", error.UTF8String ?: "",
                                   AstralJSONFromObject(Environment()));
}

void AstralRecordUnexpectedExit(NSString *pluginID, NSArray<NSString *> *running)
{
    astral::reports::unexpectedExit(pluginID.UTF8String ?: "", AstralJSONFromObject(Environment()), AstralJSONFromObject(running));
}

static void RecordDiagnostics(NSData *json)
{
    if (!json.length) return;
    nlohmann::json payload = nlohmann::json::parse(std::string((const char *)json.bytes, json.length), nullptr, false);
    if (!payload.is_object()) return;
    auto record = [&](const char *key, const char *kind, std::optional<std::string> (*stack)(const nlohmann::json &)) {
        auto found = payload.find(key);
        if (found == payload.end() || !found->is_array()) return;
        for (const nlohmann::json &diagnostic : *found) {
            std::optional<std::string> text = stack(diagnostic);
            if (!text) continue;
            std::string detail = astral::sanitizeDiagnosticText(*text);
            std::string firstLine = detail.substr(0, detail.find('\n'));
            astral::reports::add({{"kind", kind},
                                  {"time", NSDate.date.timeIntervalSince1970},
                                  {"title", std::string(strcmp(kind, "crash") == 0 ? "Crash: " : "Hang: ") + firstLine},
                                  {"detail", detail},
                                  {"fingerprint", astral::reports::fingerprint(detail)},
                                  {"environment", AstralJSONFromObject(Environment())}});
        }
    };
    record("crashDiagnostics", "crash", astral::metricKitCrashStack);
    record("hangDiagnostics", "hang", astral::metricKitHangStack);
}

@implementation AstralCrashSubscriber
- (void)didReceiveDiagnosticPayloads:(NSArray<MXDiagnosticPayload *> *)payloads
{
    for (MXDiagnosticPayload *payload in payloads) RecordDiagnostics(payload.JSONRepresentation);
}
@end

static void InstallImageCheck(void)
{
    std::set<std::string> loaded;
    for (uint32_t index = 0; index < _dyld_image_count(); index++) {
        const char *path = _dyld_get_image_name(index);
        if (path) loaded.insert(@(path).lastPathComponent.UTF8String);
    }
    NSString *frameworks = NSBundle.mainBundle.privateFrameworksPath;
    NSString *executable = NSBundle.mainBundle.executablePath.lastPathComponent;
    astral::setMetricKitImageCheck([loaded, frameworks, executable](const std::string &binary) {
        if (loaded.count(binary)) return true;
        NSString *name = @(binary.c_str());
        NSFileManager *files = NSFileManager.defaultManager;
        return [name isEqualToString:executable] ||
               [files fileExistsAtPath:[frameworks stringByAppendingPathComponent:[name stringByAppendingPathExtension:@"framework"]]] ||
               [files fileExistsAtPath:[frameworks stringByAppendingPathComponent:name]];
    });
}

void AstralStartCrashReports(NSArray<NSString *> * (^runningPlugins)(void))
{
    static AstralCrashSubscriber *subscriber;
    static dispatch_once_t once;
    gRunningPlugins = [runningPlugins copy];
    dispatch_once(&once, ^{
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
            InstallImageCheck();
            subscriber = [AstralCrashSubscriber new];
            MXMetricManager *manager = MXMetricManager.sharedManager;
            [manager addSubscriber:subscriber];
            [subscriber didReceiveDiagnosticPayloads:manager.pastDiagnosticPayloads];
        });
    });
}

NSArray<NSDictionary *> *AstralCrashReports(void)
{
    id reports = AstralObjectFromJSON(astral::reports::list());
    return [reports isKindOfClass:NSArray.class] ? reports : @[];
}

NSString *AstralCrashReportText(NSDictionary *report)
{
    return [NSString stringWithUTF8String:astral::reports::text(AstralJSONFromObject(report)).c_str()] ?: @"";
}

NSURL *AstralCrashReportIssueURL(NSDictionary *report)
{
    return [NSURL URLWithString:[NSString stringWithUTF8String:astral::reports::issueURL(AstralJSONFromObject(report)).c_str()] ?: @""];
}
