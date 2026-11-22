#import "Backup.h"

#import <UIKit/UIKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

#include <sstream>

#define TOML_EXCEPTIONS 0
#define TOML_HEADER_ONLY 1
#include <toml++/toml.hpp>

#include "LoaderSettings.h"
#import "AstralState.h"
#import "FilePicker.h"
#import "LuaHost.h"
#import "Marketplace.h"
#import "PhoenixImage.h"
#import "Themes.h"
#import "TomlDocument.h"

static const int kBackupVersion = 1;

static void Append(toml::table &table, const std::string &key, const nlohmann::json &value);

static toml::array ArrayOf(const nlohmann::json &values)
{
    toml::array array;
    for (const nlohmann::json &value : values) {
        if (value.is_string())
            array.push_back(value.get<std::string>());
        else if (value.is_boolean())
            array.push_back(value.get<bool>());
        else if (value.is_number_integer())
            array.push_back(value.get<int64_t>());
        else if (value.is_number())
            array.push_back(value.get<double>());
    }
    return array;
}

static toml::table TableOf(const nlohmann::json &object)
{
    toml::table table;
    if (!object.is_object()) return table;
    for (auto entry = object.begin(); entry != object.end(); ++entry) Append(table, entry.key(), entry.value());
    return table;
}

static void Append(toml::table &table, const std::string &key, const nlohmann::json &value)
{
    if (value.is_object())
        table.insert(key, TableOf(value));
    else if (value.is_array())
        table.insert(key, ArrayOf(value));
    else if (value.is_string())
        table.insert(key, value.get<std::string>());
    else if (value.is_boolean())
        table.insert(key, value.get<bool>());
    else if (value.is_number_integer())
        table.insert(key, value.get<int64_t>());
    else if (value.is_number())
        table.insert(key, value.get<double>());
}

static NSArray<NSString *> *LocalPluginIDs(void)
{
    NSMutableArray *ids = [NSMutableArray array];
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        if (![plugin[@"bundled"] boolValue]) [ids addObject:plugin[@"id"]];
    }
    return ids;
}

static NSArray<NSString *> *LocalThemeIDs(void)
{
    NSMutableArray *ids = [NSMutableArray array];
    for (NSDictionary *theme in AstralThemes()) {
        if (![theme[@"bundled"] boolValue]) [ids addObject:theme[@"id"]];
    }
    return ids;
}

NSString *AstralBackupTOML(void)
{
    nlohmann::json snapshot = AstralSettings()->snapshot();
    if (snapshot.contains("astral") && snapshot["astral"].is_object()) snapshot["astral"].erase("enabledPlugin");
    nlohmann::json backup = nlohmann::json::object();
    backup["format"] = kBackupVersion;
    backup["astral"] = AstralVersion().UTF8String;
    backup["settings"] = snapshot.contains("astral") ? snapshot["astral"] : nlohmann::json::object();
    nlohmann::json plugins_settings = nlohmann::json::object();
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        std::string identifier = [plugin[@"id"] UTF8String];
        nlohmann::json entry = {{"enabled", (bool)[plugin[@"enabled"] boolValue]}};
        nlohmann::json values = nlohmann::json::object();
        for (NSDictionary *option in plugin[@"switches"]) {
            std::string key = [option[@"key"] UTF8String];
            auto stored = AstralSettings()->get("plugins", identifier + ".setting." + key);
            if (stored) values[key] = *stored;
        }
        if (!values.empty()) entry["settings"] = values;
        plugins_settings[identifier] = entry;
    }
    backup["plugins"] = plugins_settings;
    nlohmann::json plugins = nlohmann::json::array(), themes = nlohmann::json::array();
    for (NSString *identifier in LocalPluginIDs()) plugins.push_back(identifier.UTF8String);
    for (NSString *identifier in LocalThemeIDs()) themes.push_back(identifier.UTF8String);
    backup["marketplace"] = {{"plugins", plugins}, {"themes", themes}};
    std::ostringstream stream;
    stream << TableOf(backup) << "\n";
    return [NSString stringWithUTF8String:stream.str().c_str()];
}

static nlohmann::json JSONValue(id value)
{
    if ([value isKindOfClass:NSDictionary.class]) {
        nlohmann::json object = nlohmann::json::object();
        for (NSString *key in value) object[key.UTF8String] = JSONValue(value[key]);
        return object;
    }
    if ([value isKindOfClass:NSArray.class]) {
        nlohmann::json array = nlohmann::json::array();
        for (id item in value) array.push_back(JSONValue(item));
        return array;
    }
    if ([value isKindOfClass:NSString.class]) return std::string([value UTF8String]);
    if ([value isKindOfClass:NSNumber.class]) {
        if (strcmp([value objCType], @encode(BOOL)) == 0 || strcmp([value objCType], "c") == 0) return (bool)[value boolValue];
        if (CFNumberIsFloatType((__bridge CFNumberRef)value)) return [value doubleValue];
        return (int64_t)[value longLongValue];
    }
    return nullptr;
}

static NSDictionary *CatalogEntry(NSString *kind, NSString *identifier)
{
    for (NSDictionary *entry in AstralCatalog()[[kind stringByAppendingString:@"s"]]) {
        if ([entry[@"id"] isEqualToString:identifier]) return entry;
    }
    return nil;
}

NSString *AstralApplyBackup(NSData *data, void (^changed)(void))
{
    NSDictionary *backup = AstralParseTOML(data);
    if (![backup[@"format"] isKindOfClass:NSNumber.class] || [backup[@"format"] intValue] != kBackupVersion) return @"not an astral backup";
    NSDictionary *settings = [backup[@"settings"] isKindOfClass:NSDictionary.class] ? backup[@"settings"] : @{};
    for (NSString *key in settings) AstralSettings()->set("astral", key.UTF8String, JSONValue(settings[key]));
    NSDictionary *plugins = [backup[@"plugins"] isKindOfClass:NSDictionary.class] ? backup[@"plugins"] : @{};
    for (NSString *identifier in plugins) {
        NSDictionary *entry = [plugins[identifier] isKindOfClass:NSDictionary.class] ? plugins[identifier] : @{};
        std::string prefix = identifier.UTF8String;
        if ([entry[@"enabled"] isKindOfClass:NSNumber.class])
            AstralSettings()->set("plugins", prefix + ".enabled", (bool)[entry[@"enabled"] boolValue]);
        NSDictionary *values = [entry[@"settings"] isKindOfClass:NSDictionary.class] ? entry[@"settings"] : @{};
        for (NSString *key in values) AstralSettings()->set("plugins", prefix + ".setting." + key.UTF8String, JSONValue(values[key]));
    }
    AstralSettings()->save();
    AstralApplyTheme(AstralActiveThemeID());
    NSDictionary *marketplace = [backup[@"marketplace"] isKindOfClass:NSDictionary.class] ? backup[@"marketplace"] : @{};
    NSArray *installedPlugins = LocalPluginIDs(), *installedThemes = LocalThemeIDs();
    for (NSString *kind in @[ @"plugin", @"theme" ]) {
        NSArray *wanted = [marketplace[[kind stringByAppendingString:@"s"]] isKindOfClass:NSArray.class]
                              ? marketplace[[kind stringByAppendingString:@"s"]]
                              : @[];
        NSArray *installed = [kind isEqualToString:@"plugin"] ? installedPlugins : installedThemes;
        for (NSString *identifier in wanted) {
            NSDictionary *entry =
                [identifier isKindOfClass:NSString.class] && ![installed containsObject:identifier] ? CatalogEntry(kind, identifier) : nil;
            if (entry) AstralInstallPackage(entry, kind, changed);
        }
    }
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        BOOL enabled = AstralSettings()->getBoolean("plugins", [[plugin[@"id"] stringByAppendingString:@".enabled"] UTF8String],
                                                    [plugin[@"enabled"] boolValue]);
        AstralLuaSetEnabled(plugin[@"id"], enabled);
    }
    changed();
    return nil;
}

static UIViewController *TopController(void)
{
    UIViewController *controller = AstralPhoenixWindow().rootViewController;
    while (controller.presentedViewController) controller = controller.presentedViewController;
    return controller;
}

void AstralExportBackup(void)
{
    NSString *text = AstralBackupTOML();
    NSString *path = [NSTemporaryDirectory() stringByAppendingPathComponent:@"astral-backup.toml"];
    if (![text writeToFile:path atomically:YES encoding:NSUTF8StringEncoding error:nil]) return;
    UIActivityViewController *sheet = [[UIActivityViewController alloc] initWithActivityItems:@[ [NSURL fileURLWithPath:path] ]
                                                                        applicationActivities:nil];
    UIViewController *top = TopController();
    sheet.popoverPresentationController.sourceView = top.view;
    [top presentViewController:sheet animated:YES completion:nil];
}

void AstralImportBackup(void (^changed)(void))
{
    AstralPickFile(@[ @"toml" ], ^(NSData *data, NSString *name) {
        NSString *failure = data ? AstralApplyBackup(data, changed) : @"could not read the file";
        AstralShowAlert(failure ? @"Import failed" : @"Settings imported",
                        failure ?: @"Missing marketplace plugins and themes are being installed.");
    });
}
