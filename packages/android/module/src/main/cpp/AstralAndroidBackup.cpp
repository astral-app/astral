#include "AstralAndroidBackup.h"

#include <limits>
#include <sstream>
#include <string_view>

#define TOML_EXCEPTIONS 0
#include <toml++/toml.hpp>

#include "AstralAndroidMarketplace.h"
#include "AstralLuaHost.h"
#include "AstralSettings.h"
#include "AstralThemes.h"
#include "AstralTOML.h"

namespace {

using nlohmann::json;

toml::table tableOf(const json &object);

toml::array arrayOf(const json &values)
{
    toml::array result;
    for (const auto &value : values) {
        if (value.is_object())
            result.push_back(tableOf(value));
        else if (value.is_array())
            result.push_back(arrayOf(value));
        else if (value.is_string())
            result.push_back(value.get<std::string>());
        else if (value.is_boolean())
            result.push_back(value.get<bool>());
        else if (value.is_number_unsigned() && value.get<uint64_t>() <= static_cast<uint64_t>(std::numeric_limits<int64_t>::max()))
            result.push_back(static_cast<int64_t>(value.get<uint64_t>()));
        else if (value.is_number_integer())
            result.push_back(value.get<int64_t>());
        else if (value.is_number())
            result.push_back(value.get<double>());
    }
    return result;
}

void append(toml::table &table, const std::string &key, const json &value)
{
    if (value.is_object())
        table.insert(key, tableOf(value));
    else if (value.is_array())
        table.insert(key, arrayOf(value));
    else if (value.is_string())
        table.insert(key, value.get<std::string>());
    else if (value.is_boolean())
        table.insert(key, value.get<bool>());
    else if (value.is_number_unsigned() && value.get<uint64_t>() <= static_cast<uint64_t>(std::numeric_limits<int64_t>::max()))
        table.insert(key, static_cast<int64_t>(value.get<uint64_t>()));
    else if (value.is_number_integer())
        table.insert(key, value.get<int64_t>());
    else if (value.is_number())
        table.insert(key, value.get<double>());
}

toml::table tableOf(const json &object)
{
    toml::table result;
    if (!object.is_object()) return result;
    for (auto entry = object.begin(); entry != object.end(); ++entry) append(result, entry.key(), entry.value());
    return result;
}

std::string identifier(const json &value)
{
    return value.is_object() && value.contains("id") && value["id"].is_string() ? value["id"].get<std::string>() : std::string();
}

json identifiers(const json &values, bool includeBundled)
{
    json result = json::array();
    if (!values.is_array()) return result;
    for (const auto &value : values) {
        if (value.is_object() && value.value("bundled", false) && !includeBundled) continue;
        std::string id = identifier(value);
        if (!id.empty()) result.push_back(id);
    }
    return result;
}

bool installed(const json &values, const std::string &wanted)
{
    if (!values.is_array()) return false;
    for (const auto &value : values)
        if (identifier(value) == wanted) return true;
    return false;
}

json packageIn(const json &catalog, const std::string &kind, const std::string &wanted)
{
    auto entries = catalog.find(kind + "s");
    if (entries == catalog.end() || !entries->is_array()) return nullptr;
    for (const auto &entry : *entries)
        if (identifier(entry) == wanted) return entry;
    return nullptr;
}

void installMissing(const json &wanted, const std::string &kind, const json &installedItems, const json &catalog,
                    astral::android::Marketplace &marketplace, const std::function<void()> &changed)
{
    if (!wanted.is_array()) return;
    for (const auto &entry : wanted) {
        if (!entry.is_string()) continue;
        std::string id = entry.get<std::string>();
        if (id.empty() || installed(installedItems, id)) continue;
        json package = packageIn(catalog, kind, id);
        if (!package.is_null()) marketplace.installPackage(package, kind, changed);
    }
}

}

std::string astral::android::exportBackup(SettingsStore &settings, const std::string &version, const json &plugins, const json &themes)
{
    json snapshot = settings.snapshot();
    if (snapshot.contains("astral") && snapshot["astral"].is_object()) snapshot["astral"].erase("enabledPlugin");
    json backup = json::object();
    backup["format"] = 1;
    backup["astral"] = version;
    backup["settings"] = snapshot.contains("astral") ? snapshot["astral"] : json::object();
    json pluginSettings = json::object();
    for (const auto &plugin : plugins) {
        std::string id = identifier(plugin);
        if (id.empty()) continue;
        json entry = {{"enabled", plugin.value("enabled", false)}};
        json values = json::object();
        for (const auto &setting : plugin.value("switches", json::array())) {
            if (!setting.is_object() || !setting.contains("key") || !setting["key"].is_string()) continue;
            std::string key = setting["key"].get<std::string>();
            if (key.empty()) continue;
            auto stored = settings.get("plugins", id + ".setting." + key);
            if (stored) values[key] = *stored;
        }
        if (!values.empty()) entry["settings"] = values;
        pluginSettings[id] = entry;
    }
    backup["plugins"] = std::move(pluginSettings);
    backup["marketplace"] = {{"plugins", identifiers(plugins, false)}, {"themes", identifiers(themes, false)}};
    std::ostringstream output;
    output << tableOf(backup) << "\n";
    return output.str();
}

std::optional<std::string> astral::android::importBackup(const std::string &contents, SettingsStore &settings, Themes &themes,
                                                         Marketplace &marketplace,
                                                         const std::function<void(const std::string &)> &applyTheme,
                                                         const std::function<void()> &changed)
{
    if (contents.size() > (1u << 20)) return "backup exceeds 1 MiB";
    json backup = parseTOML(contents);
    if (!backup.is_object() || !backup.contains("format") || !backup["format"].is_number_integer() || backup["format"] != 1)
        return "not an astral backup";
    const json &astralSettings = backup.value("settings", json::object());
    if (astralSettings.is_object()) {
        for (auto entry = astralSettings.begin(); entry != astralSettings.end(); ++entry) {
            if (entry.key() != "enabledPlugin") settings.set("astral", entry.key(), entry.value());
        }
    }
    const json &pluginSettings = backup.value("plugins", json::object());
    if (pluginSettings.is_object()) {
        for (auto plugin = pluginSettings.begin(); plugin != pluginSettings.end(); ++plugin) {
            if (plugin.key().empty() || plugin.key().size() > 128 || !plugin.value().is_object()) continue;
            const json &entry = plugin.value();
            if (entry.contains("enabled") && entry["enabled"].is_boolean())
                settings.set("plugins", plugin.key() + ".enabled", entry["enabled"]);
            const json &values = entry.value("settings", json::object());
            if (!values.is_object()) continue;
            for (auto value = values.begin(); value != values.end(); ++value) {
                if (!value.key().empty() && value.key().size() <= 48)
                    settings.set("plugins", plugin.key() + ".setting." + value.key(), value.value());
            }
        }
    }
    settings.save();
    if (applyTheme) applyTheme(themes.active());
    json plugins = astral::luahost::plugins();
    for (const auto &plugin : plugins) {
        std::string id = identifier(plugin);
        if (id.empty()) continue;
        bool enabled = settings.getBoolean("plugins", id + ".enabled", plugin.value("enabled", false));
        astral::luahost::setEnabled(id, enabled);
    }
    json marketplaceItems = backup.value("marketplace", json::object());
    json catalog = marketplace.catalog();
    installMissing(marketplaceItems.value("plugins", json::array()), "plugin", plugins, catalog, marketplace, changed);
    installMissing(marketplaceItems.value("themes", json::array()), "theme", themes.list(), catalog, marketplace, changed);
    if (changed) changed();
    return std::nullopt;
}
