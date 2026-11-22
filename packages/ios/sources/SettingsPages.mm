#import "NetworkFilter.h"
#import "SettingsPages.h"
#import "DevServer.h"

#include "AstralSettings.h"
#import "AstralState.h"
#import "AuthorAvatars.h"
#import "Backup.h"
#import "FilePicker.h"
#import "CrashReports.h"
#import <UIKit/UIKit.h>
#import "ImageAssets.h"
#import "DiscordCache.h"
#import "LuaHost.h"
#import "Marketplace.h"
#import "PhoenixWire.h"
#import "Themes.h"

static NSString *const kPluginRoute = @"plugin:";
static NSString *const kPluginSettingsRoute = @"plugin-settings:";

static NSDictionary *Row(NSString *key, NSString *title, NSString *icon, AstralRowKind kind)
{
    return @{@"key" : key, @"title" : title, @"icon" : icon, @"kind" : @(kind)};
}

static NSDictionary *With(NSDictionary *row, NSDictionary *extra)
{
    NSMutableDictionary *merged = [row mutableCopy];
    [merged addEntriesFromDictionary:extra];
    return merged;
}

static NSDictionary *Section(NSString *title, NSArray *rows)
{
    return @{@"title" : title, @"rows" : rows};
}

static NSDictionary *Page(NSString *title, NSArray *sections)
{
    return @{@"title" : title, @"sections" : sections};
}

NSDictionary *AstralSettingsCategory(void)
{
    return @{
        @"title" : @"astral",
        @"rows" : @[
            @{@"key" : @"general", @"title" : @"astral", @"icon" : @"res://astral_galaxy"},
            @{@"key" : @"plugins", @"title" : @"Plugins", @"icon" : @"res://ic_mana_puzzle_piece"},
            @{@"key" : @"design", @"title" : @"Design", @"icon" : @"res://ic_mana_paintbrush_thick"},
            @{@"key" : @"marketplace", @"title" : @"Marketplace", @"icon" : @"res://ic_mana_shop"},
            @{@"key" : @"updates", @"title" : @"Updates", @"icon" : @"res://ic_mana_download"},
        ]
    };
}

static NSDictionary *PluginWithID(NSString *identifier)
{
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        if ([plugin[@"id"] isEqualToString:identifier]) return plugin;
    }
    return nil;
}

static BOOL SafeMode(void)
{
    return AstralSettings()->getBoolean("astral", "safe-mode", false);
}

static NSDictionary *ReportsSection(void)
{
    NSArray<NSDictionary *> *reports = AstralCrashReports();
    NSMutableArray *rows = [NSMutableArray array];
    NSDateFormatter *formatter = [NSDateFormatter new];
    formatter.dateStyle = NSDateFormatterShortStyle;
    formatter.timeStyle = NSDateFormatterShortStyle;
    for (NSUInteger index = 0; index < reports.count && index < 5; index++) {
        NSDictionary *report = reports[index];
        NSString *icon = [report[@"kind"] isEqualToString:@"plugin"] ? @"res://ic_mana_puzzle_piece_retry" : @"res://ic_mana_bug";
        NSDate *date = [NSDate dateWithTimeIntervalSince1970:[report[@"time"] doubleValue]];
        [rows addObject:With(Row([NSString stringWithFormat:@"report.%lu", (unsigned long)index], report[@"title"], icon, kAstralRowLink),
                             @{@"subtitle" : [NSString stringWithFormat:@"%@ · tap to copy and report", [formatter stringFromDate:date]]})];
    }
    if (!rows.count) [rows addObject:Row(@"no-reports", @"No crashes or plugin failures", @"res://ic_mana_circle_check", kAstralRowInfo)];
    return Section(@"Reports", rows);
}

static void OpenReport(NSUInteger index)
{
    NSArray<NSDictionary *> *reports = AstralCrashReports();
    if (index >= reports.count) return;
    UIPasteboard.generalPasteboard.string = AstralCrashReportText(reports[index]);
    NSURL *url = AstralCrashReportIssueURL(reports[index]);
    if (url) [UIApplication.sharedApplication openURL:url options:@{} completionHandler:nil];
}

static NSDictionary *GeneralPage(AstralPageContext context)
{
    AstralWireStats stats = astral_wire_stats();
    NSString *recovery = context.recovering ? @"Plugins paused after a crash" : @"Normal";
    return Page(@"astral", @[
        Section(@"astral",
                @[
                    With(Row(@"version", @"Version", @"res://ic_mana_circle_information", kAstralRowInfo), @{@"detail" : AstralVersion()}),
                    With(Row(@"discord", @"Discord", @"res://ic_mana_mobile_phone_settings", kAstralRowInfo),
                         @{@"detail" : context.discordVersion}),
                    With(Row(@"engine", @"Engine", @"res://ic_mana_bug", kAstralRowInfo), @{@"detail" : @"Phoenix"}),
                    With(Row(@"runtime", @"Plugin runtime", @"res://ic_mana_puzzle_piece", kAstralRowInfo), @{@"detail" : @"Lua 5.4"}),
                ]),
        Section(@"Preferences",
                @[
                    With(
                        Row(@"safe-mode", @"Safe mode", @"res://ic_mana_shield", kAstralRowSwitch),
                        @{@"on" : @(SafeMode()),
                          @"subtitle" : @"Start without plugins on the next launch"}),
                    With(
                        Row(@"developer-mode", @"Developer mode", @"res://ic_mana_bug", kAstralRowSwitch), @{
                            @"on" : @(AstralSettings()->getBoolean("astral", "developer-mode", false)),
                            @"subtitle" : AstralSettings()->getBoolean("astral", "developer-mode", false)
                                ? [NSString stringWithFormat:@"Pairing code %@ for astral dev", AstralDevPairingCode()]
                                : @"Run plugins that astral has not signed"
                        }),
                    With(
                        Row(@"staff-devtools", @"Staff DevTools", @"res://ic_mana_wrench", kAstralRowSwitch), @{
                            @"on" : @(AstralSettings()->getBoolean("astral", "staff-devtools", false)),
                            @"subtitle" : @"Unlock Discord's staff Dev Tools; relaunch Discord to apply"
                        }),
                    With(Row(@"force-phoenix", @"Force Phoenix", @"res://ic_mana_settings_arrow_up", kAstralRowInfo),
                         @{@"detail" : @"On", @"subtitle" : @"The React Native fallback stays off"}),
                ]),
        Section(@"Backup",
                @[
                    With(Row(@"export-backup", @"Export settings", @"res://ic_mana_download", kAstralRowLink),
                         @{@"subtitle" : @"Save astral, plugin and theme settings to a TOML file"}),
                    With(Row(@"import-backup", @"Import settings", @"res://ic_mana_refresh", kAstralRowLink),
                         @{@"subtitle" : @"Restore a backup and reinstall its marketplace items"}),
                ]),
        ReportsSection(),
        Section(@"Diagnostics",
                @[
                    With(Row(@"recovery", @"Recovery", @"res://ic_mana_refresh", kAstralRowInfo), @{@"detail" : recovery}),
                    With(Row(@"decorations", @"Decorations", @"res://ic_mana_bug", kAstralRowInfo), @{
                        @"detail" : [NSString stringWithFormat:@"%llu", stats.tagged],
                        @"subtitle" : [NSString stringWithFormat:@"%llu authors, %llu users, %llu channels resolved", stats.authors,
                                                                 stats.with_user, stats.with_channel]
                    }),
                    With(Row(@"telemetry", @"Telemetry blocked", @"res://ic_mana_shield", kAstralRowInfo), @{
                        @"detail" : [AstralNetworkFilterStats()[@"blocked"] stringValue],
                        @"subtitle" : @"Requests to Discord's analytics and Sentry dropped this launch"
                    }),
                    With(Row(@"reload", @"Reload plugins", @"res://ic_mana_refresh", kAstralRowLink),
                         @{@"subtitle" : @"Restart every enabled plugin"}),
                ]),
    ]);
}

static NSString *gPluginFilter = @"all";

static BOOL PluginEnabled(NSDictionary *plugin)
{
    return [plugin[@"enabled"] boolValue] && ![plugin[@"failed"] boolValue];
}

static BOOL MatchesFilter(NSDictionary *plugin)
{
    if ([gPluginFilter isEqualToString:@"enabled"]) return PluginEnabled(plugin);
    if ([gPluginFilter isEqualToString:@"disabled"]) return !PluginEnabled(plugin);
    return YES;
}

static NSString *PluginIcon(NSDictionary *plugin)
{
    if ([plugin[@"iconSvg"] length]) return AstralRegisterIconSVG([@"plugin_" stringByAppendingString:plugin[@"id"]], plugin[@"iconSvg"]);
    NSString *name = plugin[@"icon"];
    if ([name hasPrefix:@"res://"]) return name;
    if (![name hasSuffix:@"Icon"] || name.length <= 4) return @"res://astral_plug";
    NSString *stem = [name substringToIndex:name.length - 4];
    NSMutableString *asset = [NSMutableString stringWithString:@"res://ic_mana"];
    for (NSUInteger index = 0; index < stem.length; index++) {
        unichar character = [stem characterAtIndex:index];
        BOOL upper = [NSCharacterSet.uppercaseLetterCharacterSet characterIsMember:character];
        BOOL digit = [NSCharacterSet.decimalDigitCharacterSet characterIsMember:character];
        BOOL previousDigit = index > 0 && [NSCharacterSet.decimalDigitCharacterSet characterIsMember:[stem characterAtIndex:index - 1]];
        if (upper || (digit && !previousDigit)) [asset appendString:@"_"];
        [asset appendString:[[NSString stringWithCharacters:&character length:1] lowercaseString]];
    }
    return asset;
}

static NSArray<NSString *> *PermissionDescription(NSString *permission)
{
    NSDictionary<NSString *, NSArray<NSString *> *> *known = @{
        @"discord.guilds" : @[ @"Reads servers and their roles", @"res://ic_mana_server" ],
        @"discord.members" : @[ @"Reads server members", @"res://ic_mana_group" ],
        @"discord.permissions" : @[ @"Reads member permissions", @"res://ic_mana_shield" ],
        @"discord.messages" : @[ @"Reads messages in the cache", @"res://ic_mana_chat" ],
        @"discord.users" : @[ @"Reads user profiles", @"res://ic_mana_user" ],
        @"discord.relationships" : @[ @"Reads friend nicknames", @"res://ic_mana_friends" ],
        @"storage" : @[ @"Keeps its own data on this device", @"res://ic_mana_download" ],
    };
    return known[permission] ?: @[ [@"Unknown permission: " stringByAppendingString:permission], @"res://ic_mana_circle_information" ];
}

static NSString *AuthorNames(NSDictionary *plugin)
{
    NSMutableArray *names = [NSMutableArray array];
    for (NSDictionary *author in plugin[@"authors"]) [names addObject:author[@"name"]];
    return [names componentsJoinedByString:@", "];
}

static NSArray<NSString *> *AuthorAvatars(NSDictionary *plugin)
{
    NSMutableArray *avatars = [NSMutableArray array];
    NSArray *authors = plugin[@"authors"];
    for (NSUInteger index = 0; index < authors.count && avatars.count < 3; index++) {
        [avatars addObject:AstralAuthorAvatarURL(authors[index][@"id"], index)];
    }
    return avatars;
}

static NSDictionary *PluginCard(NSDictionary *plugin)
{
    NSMutableDictionary *card = [@{
        @"key" : [@"plugin." stringByAppendingString:plugin[@"id"]],
        @"title" : plugin[@"name"],
        @"authors" : AuthorNames(plugin),
        @"avatars" : AuthorAvatars(plugin),
        @"hasSettings" : @([plugin[@"switches"] count] > 0),
        @"description" : plugin[@"description"] ?: @"",
        @"version" : [plugin[@"signed"] boolValue] ? [@"v" stringByAppendingString:plugin[@"version"]]
                                                   : [NSString stringWithFormat:@"v%@ · unsigned", plugin[@"version"]],
        @"icon" : PluginIcon(plugin),
        @"on" : @(PluginEnabled(plugin)),
        @"failed" : plugin[@"failed"],
    } mutableCopy];
    if ([plugin[@"failed"] boolValue]) card[@"error"] = [@"Stopped after errors: " stringByAppendingString:plugin[@"error"]];
    return card;
}

static NSDictionary *PluginsPage(void)
{
    NSArray<NSDictionary *> *plugins = AstralLuaPlugins();
    NSUInteger enabled = 0;
    NSMutableArray *cards = [NSMutableArray array];
    for (NSDictionary *plugin in plugins) {
        if (PluginEnabled(plugin)) enabled++;
        if (MatchesFilter(plugin)) [cards addObject:PluginCard(plugin)];
    }
    NSArray *chips = @[
        @{
            @"key" : @"filter.all",
            @"title" : [NSString stringWithFormat:@"All %lu", (unsigned long)plugins.count],
            @"selected" : @([gPluginFilter isEqualToString:@"all"])
        },
        @{
            @"key" : @"filter.enabled",
            @"title" : [NSString stringWithFormat:@"Enabled %lu", (unsigned long)enabled],
            @"selected" : @([gPluginFilter isEqualToString:@"enabled"])
        },
        @{
            @"key" : @"filter.disabled",
            @"title" : [NSString stringWithFormat:@"Disabled %lu", (unsigned long)(plugins.count - enabled)],
            @"selected" : @([gPluginFilter isEqualToString:@"disabled"])
        },
    ];
    NSMutableDictionary *page = [Page(@"Plugins", @[]) mutableCopy];
    page[@"toolbar"] = @{@"chips" : chips, @"buttonKey" : @"browse", @"buttonIcon" : @"res://ic_mana_download"};
    page[@"cards"] = cards;
    if (!cards.count) {
        NSString *empty = plugins.count ? @"No plugins match this filter" : @"No plugins installed";
        page[@"sections"] = @[ Section(@"Plugins", @[ Row(@"none", empty, @"res://ic_mana_filters_horizontal", kAstralRowInfo) ]) ];
    }
    return page;
}

static NSDictionary *PluginPage(NSString *identifier)
{
    NSDictionary *plugin = PluginWithID(identifier);
    if (!plugin)
        return Page(@"Plugin", @[ Section(@"Plugin", @[ Row(@"missing", @"This plugin is no longer installed",
                                                            @"res://ic_mana_puzzle_piece_minus", kAstralRowInfo) ]) ]);
    NSMutableArray *sections = [NSMutableArray array];
    NSMutableArray *about = [NSMutableArray array];
    if ([plugin[@"description"] length]) {
        [about addObject:With(Row(@"description", plugin[@"description"], @"res://ic_mana_circle_information", kAstralRowInfo), @{})];
    }
    [about addObject:With(Row(@"plugin-version", @"Version", @"res://ic_mana_settings_info", kAstralRowInfo),
                          @{@"detail" : plugin[@"version"]})];
    if ([plugin[@"authors"] count]) {
        [about addObject:With(Row(@"authors", @"Authors", @"res://ic_mana_user", kAstralRowInfo), @{@"subtitle" : AuthorNames(plugin)})];
    }
    [about addObject:With(Row(@"source", @"Source", @"res://ic_mana_download", kAstralRowInfo),
                          @{@"detail" : [plugin[@"bundled"] boolValue] ? @"Bundled" : @"Marketplace"})];
    [sections addObject:Section(@"About", about)];

    NSMutableArray *permissions = [NSMutableArray array];
    for (NSString *permission in plugin[@"permissions"]) {
        NSArray *described = PermissionDescription(permission);
        [permissions
            addObject:With(Row([@"permission." stringByAppendingString:permission], described[0], described[1], kAstralRowInfo), @{})];
    }
    if (!permissions.count) {
        [permissions addObject:With(Row(@"permission.none", @"No access to Discord data", @"res://ic_mana_shield", kAstralRowInfo), @{})];
    }
    [sections addObject:Section(@"Permissions", permissions)];

    NSMutableArray *status =
        [NSMutableArray arrayWithObject:With(
                                            Row(@"enabled", @"Enabled", @"res://ic_mana_puzzle_piece", kAstralRowSwitch),
                                            @{@"on" : @([plugin[@"enabled"] boolValue] && ![plugin[@"failed"] boolValue])})];
    if ([plugin[@"failed"] boolValue]) {
        [status addObject:With(Row(@"error", @"Stopped after errors", @"res://ic_mana_bug", kAstralRowInfo),
                               @{@"subtitle" : plugin[@"error"]})];
        [status addObject:Row(@"retry", @"Try again", @"res://ic_mana_refresh", kAstralRowLink)];
    }
    [sections addObject:Section(@"Status", status)];

    NSMutableArray *manage =
        [NSMutableArray arrayWithObject:With(Row(@"reset", @"Reset settings", @"res://ic_mana_refresh", kAstralRowLink),
                                             @{@"subtitle" : @"Return every option to its default"})];
    if (![plugin[@"bundled"] boolValue]) [manage addObject:Row(@"uninstall", @"Uninstall", @"res://ic_mana_trash", kAstralRowLink)];
    [sections addObject:Section(@"Manage", manage)];
    return Page(plugin[@"name"], sections);
}

static NSString *NumberText(double value)
{
    return value == floor(value) ? [NSString stringWithFormat:@"%.0f", value] : [NSString stringWithFormat:@"%g", value];
}

static NSDictionary *NumberRow(NSDictionary *entry)
{
    double minimum = [entry[@"min"] doubleValue];
    double step = [entry[@"step"] doubleValue];
    double value = [entry[@"value"] doubleValue];
    int steps = (int)llround(([entry[@"max"] doubleValue] - minimum) / step);
    int position = (int)llround((value - minimum) / step);
    return With(
        Row([@"number." stringByAppendingString:entry[@"key"]], entry[@"title"], @"res://ic_mana_settings", kAstralRowNumber),
        @{@"detail" : NumberText(value),
          @"steps" : @(steps),
          @"position" : @(position)});
}

static NSDictionary *PluginSettingsPage(NSString *identifier)
{
    NSDictionary *plugin = PluginWithID(identifier);
    if (!plugin) return PluginPage(identifier);
    NSMutableArray *sections = [NSMutableArray array];
    NSMutableArray<NSString *> *order = [NSMutableArray array];
    NSMutableDictionary<NSString *, NSMutableArray *> *grouped = [NSMutableDictionary dictionary];
    NSMutableDictionary<NSString *, NSString *> *titles = [NSMutableDictionary dictionary];
    for (NSDictionary *entry in plugin[@"switches"]) {
        if ([entry[@"options"] count]) {
            NSString *choice = [@"choice:" stringByAppendingString:entry[@"key"]];
            NSMutableArray *rows = [NSMutableArray array];
            for (NSDictionary *option in entry[@"options"]) {
                BOOL selected = [option[@"value"] isEqualToString:entry[@"value"]];
                NSString *key = [NSString stringWithFormat:@"choice.%@.%@", entry[@"key"], option[@"value"]];
                NSMutableDictionary *row =
                    [With(Row(key, option[@"title"], @"res://ic_mana_settings", selected ? kAstralRowSelected : kAstralRowChoice), @{})
                        mutableCopy];
                if ([option[@"subtitle"] length]) row[@"subtitle"] = option[@"subtitle"];
                [rows addObject:row];
            }
            grouped[choice] = rows;
            [order addObject:choice];
            titles[choice] = entry[@"title"];
            continue;
        }
        NSString *section = [entry[@"section"] length] ? entry[@"section"] : @"Settings";
        if (!grouped[section]) {
            grouped[section] = [NSMutableArray array];
            [order addObject:section];
        }
        NSMutableDictionary *row = [entry[@"number"] boolValue] ? NumberRow(entry).mutableCopy
                                                                : [With(Row([@"setting." stringByAppendingString:entry[@"key"]],
                                                                            entry[@"title"], @"res://ic_mana_settings", kAstralRowSwitch),
                                                                        @{@"on" : entry[@"on"]}) mutableCopy];
        if ([entry[@"subtitle"] length]) row[@"subtitle"] = entry[@"subtitle"];
        [grouped[section] addObject:row];
    }
    for (NSString *section in order) [sections addObject:Section(titles[section] ?: section, grouped[section])];

    [sections addObject:Section(@"Manage", @[ With(Row(@"reset", @"Reset settings", @"res://ic_mana_refresh", kAstralRowLink),
                                                   @{@"subtitle" : @"Return every option to its default"}) ])];
    return Page(plugin[@"name"], sections);
}

static NSDictionary *DesignPage(void)
{
    NSString *active = AstralActiveThemeID();
    NSMutableArray *rows = [NSMutableArray array];
    for (NSDictionary *theme in AstralThemes()) {
        BOOL selected = [theme[@"id"] isEqualToString:active];
        NSMutableDictionary *row = [With(Row([@"theme." stringByAppendingString:theme[@"id"]], theme[@"name"],
                                             @"res://ic_mana_paint_palette", selected ? kAstralRowSelected : kAstralRowChoice),
                                         @{}) mutableCopy];
        if ([theme[@"description"] isKindOfClass:NSString.class]) row[@"subtitle"] = theme[@"description"];
        [rows addObject:row];
    }
    return Page(@"Design", @[
        Section(@"Themes", rows),
        Section(@"How themes apply",
                @[ With(Row(@"theme-note", @"Recolours Phoenix views right away", @"res://ic_mana_circle_information", kAstralRowInfo),
                        @{@"subtitle" : @"Colours change on screen as soon as you pick a theme"}) ]),
        Section(@"Get more",
                @[
                    With(Row(@"import-unbound-theme", @"Import Unbound theme", @"res://ic_mana_download", kAstralRowLink),
                         @{@"subtitle" : @"Convert an Unbound theme file into an astral theme"}),
                    Row(@"browse", @"Browse themes", @"res://ic_mana_shop", kAstralRowLink)
                ]),
    ]);
}

static NSString *InstalledVersion(NSString *identifier, NSString *kind)
{
    if ([kind isEqualToString:@"theme"]) {
        for (NSDictionary *theme in AstralThemes()) {
            if ([theme[@"id"] isEqualToString:identifier])
                return [theme[@"version"] isKindOfClass:NSString.class] ? theme[@"version"] : @"";
        }
        return nil;
    }
    return PluginWithID(identifier)[@"version"];
}

static NSDictionary *PackageRow(NSDictionary *package, NSString *kind)
{
    NSString *installed = InstalledVersion(package[@"id"], kind);
    NSString *status = AstralInstallStatus(package[@"id"]);
    if (!status) {
        if (!installed)
            status = @"Get";
        else if (AstralVersionIsNewer(package[@"version"], installed))
            status = @"Update";
        else
            status = @"Installed";
    }
    NSString *icon = [kind isEqualToString:@"theme"] ? @"res://ic_mana_paint_palette" : @"res://ic_mana_puzzle_piece_plus";
    NSString *subtitle = [package[@"description"] isKindOfClass:NSString.class] ? package[@"description"] : @"";
    if ([package[@"version"] isKindOfClass:NSString.class])
        subtitle = [NSString stringWithFormat:@"v%@ · %@", package[@"version"], subtitle];
    return With(Row([NSString stringWithFormat:@"%@.%@", kind, package[@"id"]], package[@"name"], icon, kAstralRowLink),
                @{@"detail" : status, @"subtitle" : subtitle});
}

static NSArray *CatalogStatusRows(void)
{
    switch (AstralCatalogCurrentState()) {
    case AstralCatalogLoading:
        return @[ Row(@"loading", @"Loading the catalog…", @"res://ic_mana_refresh", kAstralRowInfo) ];
    case AstralCatalogFailed:
        return @[ With(Row(@"failed", @"Could not load the catalog", @"res://ic_mana_bug", kAstralRowInfo),
                       @{@"subtitle" : AstralCatalogError() ?: @""}) ];
    default:
        return @[];
    }
}

static NSDictionary *MarketplacePage(void)
{
    NSDictionary *catalog = AstralCatalog();
    NSMutableArray *sections = [NSMutableArray array];
    NSArray *status = CatalogStatusRows();
    if (status.count) [sections addObject:Section(@"Catalog", status)];
    for (NSString *kind in @[ @"plugin", @"theme" ]) {
        NSMutableArray *rows = [NSMutableArray array];
        for (NSDictionary *package in catalog[[kind stringByAppendingString:@"s"]]) [rows addObject:PackageRow(package, kind)];
        if (rows.count) [sections addObject:Section([kind isEqualToString:@"plugin"] ? @"Plugins" : @"Themes", rows)];
    }
    [sections addObject:Section(@"Source", @[
                  With(Row(@"catalog-url", @"Catalog", @"res://ic_mana_link", kAstralRowInfo), @{@"subtitle" : AstralCatalogURL()}),
                  Row(@"refresh", @"Refresh", @"res://ic_mana_refresh", kAstralRowLink),
              ])];
    return Page(@"Marketplace", sections);
}

static NSArray<NSDictionary *> *PendingUpdates(void)
{
    NSMutableArray *pending = [NSMutableArray array];
    for (NSString *kind in @[ @"plugin", @"theme" ]) {
        for (NSDictionary *package in AstralCatalog()[[kind stringByAppendingString:@"s"]]) {
            NSString *installed = InstalledVersion(package[@"id"], kind);
            if (installed && AstralVersionIsNewer(package[@"version"], installed))
                [pending addObject:@{@"package" : package, @"kind" : kind}];
        }
    }
    return pending;
}

static NSDictionary *UpdatesPage(AstralPageContext context)
{
    NSString *latest = AstralCatalog()[@"astral"][@"version"];
    NSString *astralStatus = !latest                                         ? @"Unknown"
                             : AstralVersionIsNewer(latest, AstralVersion()) ? [@"Update to " stringByAppendingString:latest]
                                                                             : @"Up to date";
    BOOL supported = [context.compatibility isEqualToString:@"ready"];
    NSMutableArray *addonRows = [NSMutableArray array];
    for (NSDictionary *pending in PendingUpdates()) {
        NSDictionary *package = pending[@"package"];
        [addonRows addObject:With(Row([NSString stringWithFormat:@"%@.%@", pending[@"kind"], package[@"id"]], package[@"name"],
                                      @"res://ic_mana_cloud_download", kAstralRowLink),
                                  @{
                                      @"detail" : AstralInstallStatus(package[@"id"]) ?: [@"v" stringByAppendingString:package[@"version"]],
                                      @"subtitle" : [NSString
                                          stringWithFormat:@"Installed v%@", InstalledVersion(package[@"id"], pending[@"kind"])]
                                  })];
    }
    if (addonRows.count > 1) [addonRows addObject:Row(@"update-all", @"Update all", @"res://ic_mana_download", kAstralRowLink)];
    if (!addonRows.count) [addonRows addObject:Row(@"current", @"Everything is up to date", @"res://ic_mana_circle_check", kAstralRowInfo)];
    NSMutableArray *sections = [NSMutableArray array];
    NSArray *status = CatalogStatusRows();
    if (status.count) [sections addObject:Section(@"Catalog", status)];
    [sections addObjectsFromArray:@[
        Section(@"astral", @[ With(Row(@"astral", @"astral", @"res://ic_mana_settings", kAstralRowInfo),
                                   @{@"detail" : astralStatus, @"subtitle" : [@"Installed " stringByAppendingString:AstralVersion()]}) ]),
        Section(@"Discord", @[ With(Row(@"discord-build", @"Build", @"res://ic_mana_mobile_phone_settings", kAstralRowInfo),
                                    @{@"detail" : supported ? @"Supported" : @"Unsupported", @"subtitle" : context.discordVersion}) ]),
        Section(@"Plugins and themes", addonRows),
        Section(@"Check", @[ Row(@"refresh", @"Check for updates", @"res://ic_mana_refresh", kAstralRowLink) ]),
    ]];
    return Page(@"Updates", sections);
}

NSDictionary *AstralSettingsPageModel(NSString *route, AstralPageContext context)
{
    if ([route isEqualToString:@"general"]) return GeneralPage(context);
    if ([route isEqualToString:@"plugins"] || route.length == 0) return PluginsPage();
    if ([route hasPrefix:kPluginSettingsRoute]) return PluginSettingsPage([route substringFromIndex:kPluginSettingsRoute.length]);
    if ([route hasPrefix:kPluginRoute]) return PluginPage([route substringFromIndex:kPluginRoute.length]);
    if ([route isEqualToString:@"design"]) return DesignPage();
    if ([route isEqualToString:@"marketplace"]) return MarketplacePage();
    if ([route isEqualToString:@"updates"]) return UpdatesPage(context);
    return Page(@"astral", @[]);
}

static NSDictionary *CatalogPackage(NSString *kind, NSString *identifier)
{
    for (NSDictionary *package in AstralCatalog()[[kind stringByAppendingString:@"s"]]) {
        if ([package[@"id"] isEqualToString:identifier]) return package;
    }
    return nil;
}

static void InstallFromKey(NSString *key, void (^changed)(void))
{
    NSRange dot = [key rangeOfString:@"."];
    if (dot.location == NSNotFound) return;
    NSString *kind = [key substringToIndex:dot.location];
    NSString *identifier = [key substringFromIndex:dot.location + 1];
    NSDictionary *package = CatalogPackage(kind, identifier);
    NSString *installed = InstalledVersion(identifier, kind);
    if (package && (!installed || AstralVersionIsNewer(package[@"version"], installed))) AstralInstallPackage(package, kind, changed);
}

static NSString *PluginAction(NSString *identifier, NSString *key, void (^changed)(void))
{
    NSDictionary *plugin = PluginWithID(identifier);
    if (!plugin) return @"..";
    if ([key isEqualToString:@"switch.enabled"] || [key isEqualToString:@"retry"]) {
        AstralLuaSetEnabled(identifier, [plugin[@"failed"] boolValue] || ![plugin[@"enabled"] boolValue]);
    } else if ([key hasPrefix:@"switch.setting."]) {
        NSString *settingKey = [key substringFromIndex:@"switch.setting.".length];
        for (NSDictionary *entry in plugin[@"switches"]) {
            if ([entry[@"key"] isEqualToString:settingKey]) AstralLuaSetSwitch(identifier, settingKey, ![entry[@"on"] boolValue]);
        }
    } else if ([key hasPrefix:@"number."]) {
        NSString *rest = [key substringFromIndex:@"number.".length];
        NSRange equals = [rest rangeOfString:@"=" options:NSBackwardsSearch];
        if (equals.location != NSNotFound) {
            NSString *settingKey = [rest substringToIndex:equals.location];
            int position = [[rest substringFromIndex:equals.location + 1] intValue];
            for (NSDictionary *entry in plugin[@"switches"]) {
                if (![entry[@"number"] boolValue] || ![entry[@"key"] isEqualToString:settingKey]) continue;
                AstralLuaSetNumber(identifier, settingKey, [entry[@"min"] doubleValue] + position * [entry[@"step"] doubleValue]);
            }
        }
    } else if ([key hasPrefix:@"choice."]) {
        NSString *rest = [key substringFromIndex:@"choice.".length];
        NSRange dot = [rest rangeOfString:@"."];
        if (dot.location != NSNotFound) {
            AstralLuaSetChoice(identifier, [rest substringToIndex:dot.location], [rest substringFromIndex:dot.location + 1]);
        }
    } else if ([key isEqualToString:@"reset"]) {
        AstralLuaResetSettings(identifier);
    } else if ([key isEqualToString:@"uninstall"] && ![plugin[@"bundled"] boolValue]) {
        [NSFileManager.defaultManager removeItemAtPath:plugin[@"directory"] error:nil];
        AstralLuaRemoveStorage(identifier);
        AstralLuaRemove(identifier);
        AstralLuaRescan();
        changed();
        return @"..";
    }
    return nil;
}

NSString *AstralSettingsPageAction(NSString *route, NSString *key, void (^changed)(void))
{
    if ([route hasPrefix:kPluginSettingsRoute]) return PluginAction([route substringFromIndex:kPluginSettingsRoute.length], key, changed);
    if ([route hasPrefix:kPluginRoute]) return PluginAction([route substringFromIndex:kPluginRoute.length], key, changed);
    if ([key isEqualToString:@"browse"]) return @"marketplace";
    if ([key hasPrefix:@"switch.plugin."]) {
        NSString *identifier = [key substringFromIndex:@"switch.plugin.".length];
        NSDictionary *plugin = PluginWithID(identifier);
        if (plugin) AstralLuaSetEnabled(identifier, [plugin[@"failed"] boolValue] || ![plugin[@"enabled"] boolValue]);
        return nil;
    }
    if ([key hasPrefix:@"settings.plugin."])
        return [kPluginSettingsRoute stringByAppendingString:[key substringFromIndex:@"settings.plugin.".length]];
    if ([key hasPrefix:@"more.plugin."]) return [kPluginRoute stringByAppendingString:[key substringFromIndex:@"more.plugin.".length]];
    if ([key hasPrefix:@"filter."]) {
        gPluginFilter = [key substringFromIndex:@"filter.".length];
        return nil;
    }
    if ([key isEqualToString:@"import-unbound-theme"]) {
        AstralPickFile(@[ @"json" ], ^(NSData *data, NSString *name) {
            NSString *failure = data ? AstralImportUnboundTheme(data, name ?: @"Imported") : @"could not read the file";
            AstralShowAlert(failure ? @"Import failed" : @"Theme imported", failure ?: @"The theme is selected on the Design page.");
            changed();
        });
        return nil;
    }
    if ([key isEqualToString:@"export-backup"]) {
        AstralExportBackup();
        return nil;
    }
    if ([key isEqualToString:@"import-backup"]) {
        AstralImportBackup(changed);
        return nil;
    }
    if ([key hasPrefix:@"report."]) {
        OpenReport((NSUInteger)[[key substringFromIndex:@"report.".length] integerValue]);
        return nil;
    }
    if ([key isEqualToString:@"switch.developer-mode"]) {
        BOOL next = !AstralSettings()->getBoolean("astral", "developer-mode", false);
        if (AstralSettings()->set("astral", "developer-mode", (bool)next)) AstralSettings()->save();
        for (NSDictionary *plugin in AstralLuaPlugins()) {
            if ([plugin[@"enabled"] boolValue] && ![plugin[@"signed"] boolValue]) AstralLuaSetEnabled(plugin[@"id"], YES);
        }
        return nil;
    }
    if ([key isEqualToString:@"switch.staff-devtools"]) {
        bool next = !AstralSettings()->getBoolean("astral", "staff-devtools", false);
        if (AstralSettings()->set("astral", "staff-devtools", next)) AstralSettings()->save();
        return nil;
    }
    if ([key isEqualToString:@"switch.safe-mode"]) {
        if (AstralSettings()->set("astral", "safe-mode", !SafeMode())) AstralSettings()->save();
        return nil;
    }
    if ([key isEqualToString:@"reload"]) {
        for (NSDictionary *plugin in AstralLuaPlugins()) {
            if ([plugin[@"enabled"] boolValue]) AstralLuaSetEnabled(plugin[@"id"], YES);
        }
        return nil;
    }
    if ([key hasPrefix:@"theme."] && [route isEqualToString:@"design"]) {
        AstralApplyTheme([key substringFromIndex:@"theme.".length]);
        return nil;
    }
    if ([key isEqualToString:@"refresh"]) {
        AstralCatalogRefresh(changed);
        return nil;
    }
    if ([key isEqualToString:@"update-all"]) {
        for (NSDictionary *pending in PendingUpdates()) AstralInstallPackage(pending[@"package"], pending[@"kind"], changed);
        return nil;
    }
    if ([route isEqualToString:@"marketplace"] || [route isEqualToString:@"updates"]) InstallFromKey(key, changed);
    return nil;
}
