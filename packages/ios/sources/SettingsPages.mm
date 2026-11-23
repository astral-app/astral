#import "SettingsPages.h"

#import <UIKit/UIKit.h>

#include "AstralSettings.h"
#include "AstralSettingsPages.h"
#import "AstralState.h"
#import "AuthorAvatars.h"
#import "Backup.h"
#import "CrashReports.h"
#import "EmojiComposer.h"
#import "EmojiPacks.h"
#import "FilePicker.h"
#import "FoundationJSON.h"
#import "IconPacks.h"
#import "ImageAssets.h"
#import "Marketplace.h"
#import "NetworkFilter.h"
#import "SoftRestart.h"
#import "Themes.h"

static astral::SettingsPageContext gContext;

static NSString *Native(const std::string &value)
{
    return [NSString stringWithUTF8String:value.c_str()] ?: @"";
}

static std::string Plain(NSString *value)
{
    return value.UTF8String ?: "";
}

static nlohmann::json Reports(void)
{
    NSDateFormatter *formatter = [NSDateFormatter new];
    formatter.dateStyle = NSDateFormatterShortStyle;
    formatter.timeStyle = NSDateFormatterShortStyle;
    nlohmann::json reports = nlohmann::json::array();
    for (NSDictionary *report in AstralCrashReports()) {
        nlohmann::json entry = AstralJSONFromObject(report);
        if (!entry.is_object()) continue;
        entry["when"] = Plain([formatter stringFromDate:[NSDate dateWithTimeIntervalSince1970:[report[@"time"] doubleValue]]]);
        reports.push_back(entry);
    }
    return reports;
}

static void OpenReport(size_t index)
{
    NSArray<NSDictionary *> *reports = AstralCrashReports();
    if (index >= reports.count) return;
    UIPasteboard.generalPasteboard.string = AstralCrashReportText(reports[index]);
    NSURL *url = AstralCrashReportIssueURL(reports[index]);
    if (url) [UIApplication.sharedApplication openURL:url options:@{} completionHandler:nil];
}

static std::string CatalogState(void)
{
    switch (AstralCatalogCurrentState()) {
    case AstralCatalogLoading:
        return "loading";
    case AstralCatalogFailed:
        return "failed";
    case AstralCatalogReady:
        return "ready";
    default:
        return "idle";
    }
}

static void (^Block(const std::function<void()> &changed))(void)
{
    std::function<void()> copy = changed;
    return ^{
        if (copy) copy();
    };
}

static void ImportUnboundTheme(const std::function<void()> &changed)
{
    void (^done)(void) = Block(changed);
    AstralPickFile(@[ @"json" ], ^(NSData *data, NSString *name) {
        NSString *failure = data ? AstralImportUnboundTheme(data, name ?: @"Imported") : @"could not read the file";
        AstralShowAlert(failure ? @"Import failed" : @"Theme imported", failure ?: @"The theme is selected on the Design page.");
        done();
    });
}

static astral::SettingsPagesPlatform Platform(void)
{
    astral::SettingsPagesPlatform platform;
    platform.settings = AstralSettings();
    platform.version = Plain(AstralVersion());
    platform.context = [] { return gContext; };
    platform.crashReports = Reports;
    platform.openReport = OpenReport;
    platform.blockedRequests = [] { return (uint64_t)[AstralNetworkFilterStats()[@"blocked"] unsignedLongLongValue]; };
    platform.themes = [] { return AstralJSONFromObject(AstralThemes()); };
    platform.activeTheme = [] { return Plain(AstralActiveThemeID()); };
    platform.applyTheme = [](const std::string &identifier) { AstralApplyTheme(Native(identifier)); };
    platform.iconPacks = [] { return AstralJSONFromObject(AstralIconPacks()); };
    platform.activeIconPack = [] { return Plain(AstralActiveIconPackID()); };
    platform.selectIconPack = [](const std::string &identifier) {
        AstralSelectIconPack(Native(identifier));
        AstralSoftRestart();
    };
    platform.emojiPacks = [] { return AstralJSONFromObject(AstralEmojiPacks()); };
    platform.activeEmojiPack = [] { return Plain(AstralActiveEmojiPackID()); };
    platform.selectEmojiPack = [](const std::string &identifier) {
        AstralSelectEmojiPack(Native(identifier));
        AstralRepaintEmojiComposers();
    };
    platform.emojiAnimated = [] { return (bool)AstralEmojiAnimated(); };
    platform.setEmojiAnimated = [](bool animated) { AstralSetEmojiAnimated(animated); };
    platform.catalog = [] { return AstralJSONFromObject(AstralCatalog()); };
    platform.catalogState = CatalogState;
    platform.catalogError = [] { return Plain(AstralCatalogError() ?: @""); };
    platform.catalogURL = [] { return Plain(AstralCatalogURL()); };
    platform.installStatus = [](const std::string &identifier) -> std::optional<std::string> {
        NSString *status = AstralInstallStatus(Native(identifier));
        if (!status) return std::nullopt;
        return Plain(status);
    };
    platform.installPackage = [](const nlohmann::json &package, const std::string &kind, const std::function<void()> &changed) {
        AstralInstallPackage(AstralObjectFromJSON(package), Native(kind), Block(changed));
    };
    platform.refreshCatalog = [](const std::function<void()> &changed) { AstralCatalogRefresh(Block(changed)); };
    platform.authorAvatar = [](const std::string &userID, size_t index) { return Plain(AstralAuthorAvatarURL(Native(userID), index)); };
    platform.pluginIconSVG = [](const nlohmann::json &plugin) -> std::optional<std::string> {
        std::string svg = plugin.value("iconSvg", "");
        if (svg.empty()) return std::nullopt;
        return Plain(AstralRegisterIconSVG(Native("plugin_" + plugin.value("id", "")), Native(svg)));
    };
    platform.importUnboundTheme = ImportUnboundTheme;
    platform.exportBackup = [] { AstralExportBackup(); };
    platform.importBackup = [](const std::function<void()> &changed) { AstralImportBackup(Block(changed)); };
    platform.softRestart = [] { AstralSoftRestart(); };
    return platform;
}

static void Configure(AstralPageContext context)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        astral::pages::configure(Platform());
    });
    gContext = {(bool)context.recovering, Plain(context.discordVersion), Plain(context.compatibility)};
}

NSDictionary *AstralSettingsCategory(void)
{
    return AstralObjectFromJSON(astral::pages::category());
}

NSDictionary *AstralSettingsPageModel(NSString *route, AstralPageContext context)
{
    Configure(context);
    return AstralObjectFromJSON(astral::pages::model(Plain(route))) ?: @{};
}

NSString *AstralSettingsPageAction(NSString *route, NSString *key, void (^changed)(void))
{
    Configure({gContext.recovering, Native(gContext.discordVersion), Native(gContext.compatibility)});
    void (^copied)(void) = [changed copy];
    auto next = astral::pages::action(Plain(route), Plain(key), [copied] { copied(); });
    return next ? Native(*next) : nil;
}
