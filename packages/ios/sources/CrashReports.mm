#import "CrashReports.h"

#import <MetricKit/MetricKit.h>
#import <mach-o/dyld.h>

#include <set>
#include <string>

#include "AstralMetricKitCrash.h"
#import "AstralState.h"
#import "PhoenixImage.h"

static const NSUInteger kMaxReports = 20;
static const NSUInteger kMaxIssueBody = 6000;
static NSString *const kIssueURL = @"https://github.com/astral-app/astral/issues/new";

static NSArray<NSString *> * (^gRunningPlugins)(void);
static NSLock *gLock;

@interface AstralCrashSubscriber : NSObject <MXMetricManagerSubscriber>
@end

static NSString *ReportsPath(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"reports.json"];
}

static NSMutableArray<NSDictionary *> *LoadReports(void)
{
    NSData *data = [NSData dataWithContentsOfFile:ReportsPath()];
    id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    return [parsed isKindOfClass:NSArray.class] ? [parsed mutableCopy] : [NSMutableArray array];
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
    return @{
        @"astral" : AstralVersion(),
        @"discord" : [NSString stringWithFormat:@"%@ (%@)", info[@"CFBundleShortVersionString"] ?: @"?", info[@"CFBundleVersion"] ?: @"?"],
        @"engine" : @"Phoenix",
        @"phoenix" : AstralPhoenixUUID() ?: @"",
        @"device" : UIDevice.currentDevice.model ?: @"",
        @"architecture" : architecture,
        @"system" : [NSString stringWithFormat:@"%@ %@", UIDevice.currentDevice.systemName, UIDevice.currentDevice.systemVersion],
        @"locale" : NSLocale.preferredLanguages.firstObject ?: @"",
        @"plugins" : gRunningPlugins ? gRunningPlugins() : @[],
    };
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
    NSMutableDictionary *report = [@{
        @"kind" : @"plugin",
        @"time" : @(NSDate.date.timeIntervalSince1970),
        @"plugin" : pluginID,
        @"title" : [NSString stringWithFormat:@"%@ stopped after errors", pluginName],
        @"detail" : error,
        @"fingerprint" : Fingerprint([pluginID stringByAppendingString:error]),
    } mutableCopy];
    [report addEntriesFromDictionary:@{@"environment" : Environment()}];
    AddReport(report);
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
            NSString *detail = [NSString stringWithUTF8String:text->c_str()] ?: @"";
            NSString *firstLine = [detail componentsSeparatedByString:@"\n"].firstObject ?: @(kind);
            AddReport(@{
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
