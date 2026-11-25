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
#import "AstralState.h"
#import "PhoenixImage.h"

static const NSUInteger kMaxReports = 20;
static const NSUInteger kMaxIssueBody = 6000;
static NSString *const kIssueURL = @"https://github.com/astral-app/astral/issues/new";

static NSArray<NSString *> * (^gRunningPlugins)(void);
static NSLock *gLock;
static NSString *Fingerprint(NSString *text);

@interface AstralCrashSubscriber : NSObject <MXMetricManagerSubscriber>
@end

static NSString *ReportsPath(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"reports.json"];
}

static NSString *SanitizedText(id value)
{
    if (![value isKindOfClass:NSString.class]) return @"";
    std::string sanitized = astral::sanitizeDiagnosticText([value UTF8String] ?: "");
    return [NSString stringWithUTF8String:sanitized.c_str()] ?: @"";
}

static NSDictionary *SanitizedEnvironment(id value)
{
    NSDictionary *source = [value isKindOfClass:NSDictionary.class] ? value : @{};
    NSMutableDictionary *environment = [@{@"schema" : @2, @"platform" : @"ios", @"install_mode" : @"unknown"} mutableCopy];
    for (NSString *key in @[ @"device", @"architecture", @"abi", @"system", @"locale", @"astral", @"discord", @"phoenix", @"engine" ]) {
        if ([source[key] isKindOfClass:NSString.class]) environment[key] = SanitizedText(source[key]);
    }
    if ([source[@"install_mode"] isKindOfClass:NSString.class]) environment[@"install_mode"] = SanitizedText(source[@"install_mode"]);
    if ([source[@"plugins"] isKindOfClass:NSArray.class]) {
        NSMutableArray<NSString *> *plugins = [NSMutableArray array];
        for (id identifier in source[@"plugins"]) {
            if ([identifier isKindOfClass:NSString.class]) [plugins addObject:SanitizedText(identifier)];
        }
        environment[@"plugins"] = plugins;
    }
    return environment;
}

static NSMutableArray<NSDictionary *> *LoadReports(void)
{
    NSData *data = [NSData dataWithContentsOfFile:ReportsPath()];
    id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    if (![parsed isKindOfClass:NSArray.class]) return [NSMutableArray array];
    NSMutableArray<NSDictionary *> *reports = [NSMutableArray array];
    for (id value in (NSArray *)parsed) {
        if (![value isKindOfClass:NSDictionary.class]) continue;
        NSDictionary *saved = value;
        NSString *kind = [saved[@"kind"] isKindOfClass:NSString.class] ? saved[@"kind"] : @"plugin";
        if (![@[ @"plugin", @"crash", @"hang" ] containsObject:kind]) continue;
        NSString *detail = SanitizedText(saved[@"detail"] ?: saved[@"error"]);
        NSString *title = SanitizedText(saved[@"title"]);
        if (title.length == 0) title = kind;
        NSMutableDictionary *report = [@{
            @"schema" : @2,
            @"kind" : kind,
            @"title" : title,
            @"detail" : detail,
            @"environment" : SanitizedEnvironment(saved[@"environment"])
        } mutableCopy];
        for (NSString *key in @[ @"id", @"plugin" ]) {
            if ([saved[key] isKindOfClass:NSString.class]) report[key] = SanitizedText(saved[key]);
        }
        if ([saved[@"time"] isKindOfClass:NSNumber.class]) report[@"time"] = saved[@"time"];
        NSString *fingerprint = [saved[@"fingerprint"] isKindOfClass:NSString.class] ? SanitizedText(saved[@"fingerprint"]) : nil;
        report[@"fingerprint"] = fingerprint ?: Fingerprint([title stringByAppendingString:detail]);
        [reports addObject:report];
        if (reports.count == kMaxReports) break;
    }
    if (![parsed isEqualToArray:reports]) {
        NSData *sanitized = [NSJSONSerialization dataWithJSONObject:reports options:0 error:nil];
        [sanitized writeToFile:ReportsPath() options:NSDataWritingAtomic error:nil];
    }
    return reports;
}

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

static void AddReport(NSDictionary *report)
{
    [gLock lock];
    NSMutableArray *reports = LoadReports();
    for (NSDictionary *existing in reports) {
        if ([existing[@"fingerprint"] isEqualToString:report[@"fingerprint"]]) {
            [gLock unlock];
            return;
        }
    }
    [reports insertObject:report atIndex:0];
    while (reports.count > kMaxReports) [reports removeLastObject];
    [[NSJSONSerialization dataWithJSONObject:reports options:0 error:nil] writeToFile:ReportsPath() atomically:YES];
    [gLock unlock];
}

static NSString *Fingerprint(NSString *text)
{
    return [NSString stringWithFormat:@"%016llx", (unsigned long long)text.hash];
}

void AstralRecordPluginFailure(NSString *pluginID, NSString *pluginName, NSString *error)
{
    std::string detail = astral::sanitizeDiagnosticText(error.UTF8String ?: "");
    NSString *sanitized = [NSString stringWithUTF8String:detail.c_str()] ?: @"Plugin failure";
    std::string name = astral::sanitizeDiagnosticText(pluginName.UTF8String ?: "Plugin");
    NSString *sanitizedName = [NSString stringWithUTF8String:name.c_str()] ?: @"Plugin";
    NSMutableDictionary *report = [@{
        @"schema" : @2,
        @"kind" : @"plugin",
        @"time" : @(NSDate.date.timeIntervalSince1970),
        @"id" : pluginID,
        @"plugin" : pluginID,
        @"title" : [NSString stringWithFormat:@"%@ stopped after errors", sanitizedName],
        @"detail" : sanitized,
        @"fingerprint" : Fingerprint([pluginID stringByAppendingString:sanitized]),
    } mutableCopy];
    [report addEntriesFromDictionary:@{@"environment" : Environment()}];
    AddReport(report);
}

void AstralRecordUnexpectedExit(NSString *pluginID, NSArray<NSString *> *running)
{
    NSMutableDictionary *environment = [Environment() mutableCopy];
    environment[@"plugins"] = running;
    AddReport(@{
        @"schema" : @2,
        @"kind" : @"crash",
        @"time" : @(NSDate.date.timeIntervalSince1970),
        @"id" : pluginID,
        @"plugin" : pluginID,
        @"title" : [NSString stringWithFormat:@"Discord closed while %@ was running", pluginID],
        @"detail" : @"Discord ended unexpectedly during a call into this plugin.",
        @"fingerprint" : Fingerprint([@"exit:" stringByAppendingString:pluginID]),
        @"environment" : environment,
    });
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
            std::string sanitized = astral::sanitizeDiagnosticText(*text);
            NSString *detail = [NSString stringWithUTF8String:sanitized.c_str()] ?: @"";
            NSString *firstLine = [detail componentsSeparatedByString:@"\n"].firstObject ?: @(kind);
            AddReport(@{
                @"schema" : @2,
                @"kind" : @(kind),
                @"time" : @(NSDate.date.timeIntervalSince1970),
                @"title" : [NSString stringWithFormat:@"%s: %@", strcmp(kind, "crash") == 0 ? "Crash" : "Hang", firstLine],
                @"detail" : detail,
                @"fingerprint" : Fingerprint(detail),
                @"environment" : Environment(),
            });
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
        gLock = [NSLock new];
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
    [gLock lock];
    NSArray *reports = LoadReports();
    [gLock unlock];
    return reports;
}

NSString *AstralCrashReportText(NSDictionary *report)
{
    NSDictionary *environment = report[@"environment"];
    NSMutableString *text = [NSMutableString string];
    [text appendFormat:@"%@\n\n", report[@"title"]];
    [text appendFormat:@"| | |\n| --- | --- |\n| Kind | %@ |\n| astral | %@ |\n| Discord | %@ |\n| Engine | %@ |\n| Phoenix | %@ |\n| "
                       @"Device | %@ |\n| Architecture | %@ |\n| System | %@ |\n| Locale | %@ |\n",
                       report[@"kind"], environment[@"astral"], environment[@"discord"], environment[@"engine"], environment[@"phoenix"],
                       environment[@"device"], environment[@"architecture"], environment[@"system"], environment[@"locale"]];
    if (report[@"plugin"]) [text appendFormat:@"| Plugin | %@ |\n", report[@"plugin"]];
    NSArray *plugins = environment[@"plugins"];
    if (plugins.count) [text appendFormat:@"| Running plugins | %@ |\n", [plugins componentsJoinedByString:@", "]];
    [text appendFormat:@"\n```text\n%@\n```\n", report[@"detail"]];
    return text;
}

NSURL *AstralCrashReportIssueURL(NSDictionary *report)
{
    NSString *body = AstralCrashReportText(report);
    if (body.length > kMaxIssueBody)
        body = [[body substringToIndex:kMaxIssueBody] stringByAppendingString:@"\n```\n(truncated; the full report is on the clipboard)"];
    NSURLComponents *components = [NSURLComponents componentsWithString:kIssueURL];
    components.queryItems = @[
        [NSURLQueryItem queryItemWithName:@"title" value:report[@"title"]], [NSURLQueryItem queryItemWithName:@"body" value:body],
        [NSURLQueryItem queryItemWithName:@"labels" value:report[@"kind"]]
    ];
    return components.URL;
}
