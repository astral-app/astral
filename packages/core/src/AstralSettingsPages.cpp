#include "AstralSettingsPages.h"

#include <cctype>
#include <cmath>
#include <ctime>
#include <filesystem>
#include <map>
#include <mutex>
#include <vector>

#include "AstralDevServer.h"
#include "AstralLuaHost.h"
#include "AstralSettings.h"

namespace {

using nlohmann::json;

const std::string kPluginRoute = "plugin:";
const std::string kPluginSettingsRoute = "plugin-settings:";

astral::SettingsPagesPlatform gPlatform;
std::mutex gFilterMutex;
std::string gPluginFilter = "all";

json objects(json value)
{
    if (value.is_object()) return json::array({std::move(value)});
    json result = json::array();
    if (value.is_array())
        for (auto &entry : value)
            if (entry.is_object()) result.push_back(std::move(entry));
    return result;
}

json row(const std::string &key, const std::string &title, const std::string &icon, AstralRowKind kind)
{
    return {{"key", key}, {"title", title}, {"icon", icon}, {"kind", (int)kind}};
}

json with(json base, const json &extra)
{
    for (auto entry = extra.begin(); entry != extra.end(); ++entry) base[entry.key()] = entry.value();
    return base;
}

json section(const std::string &title, json rows)
{
    return {{"title", title}, {"rows", objects(std::move(rows))}};
}

json page(const std::string &title, json sections)
{
    return {{"title", title}, {"sections", objects(std::move(sections))}};
}

std::string text(const json &object, const char *key, const std::string &fallback = "")
{
    auto field = object.find(key);
    return field != object.end() && field->is_string() ? field->get<std::string>() : fallback;
}

bool flag(const json &object, const char *key)
{
    auto field = object.find(key);
    return field != object.end() && field->is_boolean() && field->get<bool>();
}

double number(const json &object, const char *key)
{
    auto field = object.find(key);
    return field != object.end() && field->is_number() ? field->get<double>() : 0;
}

json list(const std::function<json()> &source)
{
    if (!source) return json::array();
    json value = source();
    return value.is_array() ? value : json::array();
}

std::string call(const std::function<std::string()> &source)
{
    return source ? source() : std::string();
}

bool setting(const char *key)
{
    return gPlatform.settings && gPlatform.settings->getBoolean("astral", key, false);
}

void toggle(const char *key)
{
    if (!gPlatform.settings) return;
    if (gPlatform.settings->set("astral", key, !setting(key))) gPlatform.settings->save();
}

json pluginWithID(const std::string &identifier)
{
    for (const auto &plugin : astral::luahost::plugins())
        if (text(plugin, "id") == identifier) return plugin;
    return nullptr;
}

std::string reportTime(const json &report)
{
    std::string when = text(report, "when");
    if (!when.empty()) return when;
    std::time_t time = (std::time_t)number(report, "time");
    char buffer[64];
    std::strftime(buffer, sizeof(buffer), "%Y-%m-%d %H:%M", std::localtime(&time));
    return buffer;
}

json reportsSection()
{
    json reports = list(gPlatform.crashReports);
    json rows = json::array();
    for (size_t index = 0; index < reports.size() && index < 5; index++) {
        const json &report = reports[index];
        std::string icon = text(report, "kind") == "plugin" ? "res://ic_mana_puzzle_piece_retry" : "res://ic_mana_bug";
        rows.push_back(with(row("report." + std::to_string(index), text(report, "title"), icon, kAstralRowLink),
                            {{"subtitle", reportTime(report) + " · tap to copy and report"}}));
    }
    if (rows.empty()) rows.push_back(row("no-reports", "No crashes or plugin failures", "res://ic_mana_circle_check", kAstralRowInfo));
    return section("Reports", rows);
}

json generalPage(const astral::SettingsPageContext &context)
{
    AstralWireStats stats = astral_wire_stats();
    bool developer = setting("developer-mode");
    std::string blocked = std::to_string(gPlatform.blockedRequests ? gPlatform.blockedRequests() : 0);
    return page(
        "astral",
        {section(
             "astral",
             {with(row("version", "Version", "res://ic_mana_circle_information", kAstralRowInfo), {{"detail", gPlatform.version}}),
              with(row("discord", "Discord", "res://ic_mana_mobile_phone_settings", kAstralRowInfo), {{"detail", context.discordVersion}}),
              with(row("engine", "Engine", "res://ic_mana_settings", kAstralRowInfo), {{"detail", "Phoenix"}}),
              with(row("runtime", "Plugin runtime", "res://ic_mana_puzzle_piece", kAstralRowInfo), {{"detail", "Lua 5.4"}})}),
         section("Preferences",
                 {with(row("safe-mode", "Safe mode", "res://ic_mana_shield", kAstralRowSwitch),
                       {{"on", setting("safe-mode")}, {"subtitle", "Start without plugins on the next launch"}}),
                  with(row("developer-mode", "Developer mode", "res://ic_mana_bug", kAstralRowSwitch),
                       {{"on", developer},
                        {"subtitle", developer ? "Pairing code " + astral::dev::pairingCode() + " for astral dev"
                                               : std::string("Run plugins that astral has not signed")}}),
                  with(row("staff-devtools", "Staff DevTools", "res://ic_mana_wrench", kAstralRowSwitch),
                       {{"on", setting("staff-devtools")},
                        {"subtitle", gPlatform.restartGatewaySession ? "Unlock Discord's staff DevTools"
                                                                     : "Unlock Discord's staff DevTools; relaunch Discord to apply"}}),
                  with(row("force-phoenix", "Force Phoenix", "res://ic_mana_settings_arrow_up", kAstralRowInfo),
                       {{"detail", "On"}, {"subtitle", "The React Native fallback stays off"}})}),
         section("Backup", {with(row("export-backup", "Export settings", "res://ic_mana_download", kAstralRowLink),
                                 {{"subtitle", "Save astral and addon settings to a TOML file"}}),
                            with(row("import-backup", "Import settings", "res://ic_mana_refresh", kAstralRowLink),
                                 {{"subtitle", "Restore a backup and reinstall its marketplace addons"}})}),
         reportsSection(),
         section("Diagnostics",
                 {with(row("recovery", "Recovery", "res://ic_mana_refresh", kAstralRowInfo),
                       {{"detail", context.recovering ? "Plugins paused after a crash" : "Normal"}}),
                  with(row("decorations", "Decorations", "res://ic_mana_bug", kAstralRowInfo),
                       {{"detail", std::to_string(stats.tagged)},
                        {"subtitle", std::to_string(stats.authors) + " authors, " + std::to_string(stats.with_user) + " users, " +
                                         std::to_string(stats.with_channel) + " channels resolved"}}),
                  with(row("telemetry", "Telemetry blocked", "res://ic_mana_shield", kAstralRowInfo),
                       {{"detail", blocked}, {"subtitle", "Requests to Discord's analytics and Sentry dropped this launch"}}),
                  with(row("reload", "Reload plugins", "res://ic_mana_refresh", kAstralRowLink),
                       {{"subtitle", "Restart every enabled plugin"}}),
                  with(row("soft-restart", "Soft restart", "res://ic_mana_refresh", kAstralRowLink),
                       {{"subtitle", "Restart astral's runtime, theme and icons without relaunching Discord"}})})});
}

bool pluginEnabled(const json &plugin)
{
    return flag(plugin, "enabled") && !flag(plugin, "failed");
}

std::string filter()
{
    std::lock_guard<std::mutex> lock(gFilterMutex);
    return gPluginFilter;
}

bool matchesFilter(const json &plugin, const std::string &current)
{
    if (current == "enabled") return pluginEnabled(plugin);
    if (current == "disabled") return !pluginEnabled(plugin);
    return true;
}

std::string manaAsset(const std::string &name)
{
    if (name.rfind("res://", 0) == 0) return name;
    if (name.size() <= 4 || name.compare(name.size() - 4, 4, "Icon") != 0) return "res://astral_plug";
    std::string stem = name.substr(0, name.size() - 4), asset = "res://ic_mana";
    for (size_t index = 0; index < stem.size(); index++) {
        unsigned char character = (unsigned char)stem[index];
        bool upper = std::isupper(character), digit = std::isdigit(character);
        bool previousDigit = index > 0 && std::isdigit((unsigned char)stem[index - 1]);
        if (upper || (digit && !previousDigit)) asset += '_';
        asset += (char)std::tolower(character);
    }
    return asset;
}

std::string pluginIcon(const json &plugin)
{
    if (!text(plugin, "iconSvg").empty() && gPlatform.pluginIconSVG) {
        if (auto registered = gPlatform.pluginIconSVG(plugin)) return *registered;
    }
    return manaAsset(text(plugin, "icon"));
}

std::pair<std::string, std::string> permissionDescription(const std::string &permission)
{
    static const std::map<std::string, std::pair<std::string, std::string>> known = {
        {"discord.guilds", {"Reads servers and their roles", "res://ic_mana_server"}},
        {"discord.members", {"Reads server members", "res://ic_mana_group"}},
        {"discord.permissions", {"Reads member permissions", "res://ic_mana_shield"}},
        {"discord.messages", {"Reads messages in the cache", "res://ic_mana_chat"}},
        {"discord.messages.content", {"Reads cached message text", "res://ic_mana_chat"}},
        {"discord.messages.send", {"Transforms message text before sending", "res://ic_mana_chat"}},
        {"messages.display", {"Changes how message text is shown on this device", "res://ic_mana_chat"}},
        {"events.messages", {"Receives new message metadata while Discord is open", "res://ic_mana_chat"}},
        {"events.messages.filter", {"Reads and filters incoming messages before Discord handles them", "res://ic_mana_chat"}},
        {"events.lifecycle", {"Receives app foreground and background events", "res://ic_mana_calendar"}},
        {"ui.clipboard", {"Copies text to the system clipboard", "res://ic_mana_copy"}},
        {"ui.composer", {"Reads and styles draft text in the message composer", "res://ic_mana_chat"}},
        {"ui.message_background", {"Styles Phoenix message row backgrounds", "res://ic_mana_chat"}},
        {"ui.haptics", {"Triggers haptic feedback", "res://ic_mana_bell"}},
        {"ui.toasts", {"Shows brief notifications", "res://ic_mana_bell"}},
        {"ui.notifications", {"Posts system notifications", "res://ic_mana_bell"}},
        {"audio", {"Plays sounds and reads text aloud", "res://ic_mana_bell"}},
        {"ui.message_actions", {"Adds actions to the message menu", "res://ic_mana_chat"}},
        {"ui.web_auth", {"Opens a secure sign-in window", "res://ic_mana_link"}},
        {"discord.users", {"Reads user profiles", "res://ic_mana_user"}},
        {"discord.relationships", {"Reads friend nicknames", "res://ic_mana_friends"}},
        {"network", {"Makes HTTPS requests", "res://ic_mana_link"}},
        {"storage", {"Keeps its own data on this device", "res://ic_mana_download"}},
    };
    auto found = known.find(permission);
    if (found != known.end()) return found->second;
    return {"Unknown permission: " + permission, "res://ic_mana_circle_information"};
}

std::string authorNames(const json &plugin)
{
    std::string names;
    for (const auto &author : plugin.value("authors", json::array())) {
        if (!names.empty()) names += ", ";
        names += text(author, "name");
    }
    return names;
}

json authorAvatars(const json &plugin)
{
    json avatars = json::array();
    json authors = plugin.value("authors", json::array());
    for (size_t index = 0; index < authors.size() && avatars.size() < 3; index++) {
        std::string id = text(authors[index], "id");
        avatars.push_back(gPlatform.authorAvatar ? gPlatform.authorAvatar(id, index)
                                                 : "https://cdn.discordapp.com/embed/avatars/" + std::to_string(index % 6) + ".png");
    }
    return avatars;
}

json pluginCard(const json &plugin)
{
    std::string version = text(plugin, "version");
    json card = {{"key", "plugin." + text(plugin, "id")},
                 {"title", text(plugin, "name")},
                 {"authors", authorNames(plugin)},
                 {"avatars", authorAvatars(plugin)},
                 {"hasSettings", plugin.value("switches", json::array()).size() > 0},
                 {"description", text(plugin, "description")},
                 {"version", flag(plugin, "signed") ? "v" + version : "v" + version + " · unsigned"},
                 {"icon", pluginIcon(plugin)},
                 {"on", pluginEnabled(plugin)},
                 {"failed", flag(plugin, "failed")}};
    if (flag(plugin, "failed")) card["error"] = "Stopped after errors: " + text(plugin, "error");
    return card;
}

json pluginsPage()
{
    json plugins = astral::luahost::plugins();
    std::string current = filter();
    size_t enabled = 0;
    json cards = json::array();
    for (const auto &plugin : plugins) {
        if (pluginEnabled(plugin)) enabled++;
        if (matchesFilter(plugin, current)) cards.push_back(pluginCard(plugin));
    }
    json chips = {{{"key", "filter.all"}, {"title", "All " + std::to_string(plugins.size())}, {"selected", current == "all"}},
                  {{"key", "filter.enabled"}, {"title", "Enabled " + std::to_string(enabled)}, {"selected", current == "enabled"}},
                  {{"key", "filter.disabled"},
                   {"title", "Disabled " + std::to_string(plugins.size() - enabled)},
                   {"selected", current == "disabled"}}};
    json result = page("Plugins", json::array());
    result["toolbar"] = {{"chips", chips}, {"buttonKey", "browse"}, {"buttonIcon", "res://ic_mana_download"}};
    result["cards"] = cards;
    if (cards.empty()) {
        std::string empty = plugins.empty() ? "No plugins installed" : "No plugins match this filter";
        result["sections"] = {section("Plugins", {row("none", empty, "res://ic_mana_filters_horizontal", kAstralRowInfo)})};
    }
    return result;
}

json pluginPage(const std::string &identifier)
{
    json plugin = pluginWithID(identifier);
    if (plugin.is_null())
        return page("Plugin", {section("Plugin", {row("missing", "This plugin is no longer installed", "res://ic_mana_puzzle_piece_minus",
                                                      kAstralRowInfo)})});
    json sections = json::array();
    json about = json::array();
    if (!text(plugin, "description").empty())
        about.push_back(row("description", text(plugin, "description"), "res://ic_mana_circle_information", kAstralRowInfo));
    about.push_back(
        with(row("plugin-version", "Version", "res://ic_mana_settings_info", kAstralRowInfo), {{"detail", text(plugin, "version")}}));
    if (!plugin.value("authors", json::array()).empty())
        about.push_back(with(row("authors", "Authors", "res://ic_mana_user", kAstralRowInfo), {{"subtitle", authorNames(plugin)}}));
    about.push_back(with(row("source", "Source", "res://ic_mana_download", kAstralRowInfo),
                         {{"detail", flag(plugin, "bundled") ? "Bundled" : "Marketplace"}}));
    sections.push_back(section("About", about));

    json permissions = json::array();
    for (const auto &permission : plugin.value("permissions", json::array())) {
        if (!permission.is_string()) continue;
        auto described = permissionDescription(permission.get<std::string>());
        permissions.push_back(row("permission." + permission.get<std::string>(), described.first, described.second, kAstralRowInfo));
    }
    if (permissions.empty())
        permissions.push_back(row("permission.none", "No access to Discord data", "res://ic_mana_shield", kAstralRowInfo));
    sections.push_back(section("Permissions", permissions));

    json status = {with(row("enabled", "Enabled", "res://ic_mana_puzzle_piece", kAstralRowSwitch), {{"on", pluginEnabled(plugin)}})};
    if (flag(plugin, "failed")) {
        status.push_back(
            with(row("error", "Stopped after errors", "res://ic_mana_bug", kAstralRowInfo), {{"subtitle", text(plugin, "error")}}));
        status.push_back(row("retry", "Try again", "res://ic_mana_refresh", kAstralRowLink));
    }
    sections.push_back(section("Status", status));

    json manage = {with(row("reset", "Reset settings", "res://ic_mana_refresh", kAstralRowLink),
                        {{"subtitle", "Return every option to its default"}})};
    if (!flag(plugin, "bundled")) manage.push_back(row("uninstall", "Uninstall", "res://ic_mana_trash", kAstralRowLink));
    sections.push_back(section("Manage", manage));
    return page(text(plugin, "name"), sections);
}

std::string numberText(double value)
{
    char buffer[32];
    if (value == std::floor(value))
        snprintf(buffer, sizeof(buffer), "%.0f", value);
    else
        snprintf(buffer, sizeof(buffer), "%g", value);
    return buffer;
}

json numberRow(const json &entry)
{
    double minimum = number(entry, "min"), step = number(entry, "step"), value = number(entry, "value");
    int steps = (int)std::llround((number(entry, "max") - minimum) / step);
    int position = (int)std::llround((value - minimum) / step);
    return with(row("number." + text(entry, "key"), text(entry, "title"), "res://ic_mana_settings", kAstralRowNumber),
                {{"detail", numberText(value)}, {"steps", steps}, {"position", position}});
}

json pluginSettingsPage(const std::string &identifier)
{
    json plugin = pluginWithID(identifier);
    if (plugin.is_null()) return pluginPage(identifier);
    std::vector<std::string> order;
    std::map<std::string, json> grouped;
    std::map<std::string, std::string> titles;
    for (const auto &entry : plugin.value("switches", json::array())) {
        json options = entry.value("options", json::array());
        if (!options.empty()) {
            std::string choice = "choice:" + text(entry, "key");
            json rows = json::array();
            for (const auto &option : options) {
                bool selected = text(option, "value") == text(entry, "value");
                json choiceRow = row("choice." + text(entry, "key") + "." + text(option, "value"), text(option, "title"),
                                     "res://ic_mana_settings", selected ? kAstralRowSelected : kAstralRowChoice);
                if (!text(option, "subtitle").empty()) choiceRow["subtitle"] = text(option, "subtitle");
                rows.push_back(choiceRow);
            }
            grouped[choice] = rows;
            order.push_back(choice);
            titles[choice] = text(entry, "title");
            continue;
        }
        std::string name = text(entry, "section", "Settings");
        if (name.empty()) name = "Settings";
        if (!grouped.count(name)) {
            grouped[name] = json::array();
            order.push_back(name);
        }
        json settingRow = flag(entry, "number")
                              ? numberRow(entry)
                              : with(row("setting." + text(entry, "key"), text(entry, "title"), "res://ic_mana_settings", kAstralRowSwitch),
                                     {{"on", flag(entry, "on")}});
        if (!text(entry, "subtitle").empty()) settingRow["subtitle"] = text(entry, "subtitle");
        grouped[name].push_back(settingRow);
    }
    for (const auto &entry : plugin.value("textSettings", json::array())) {
        std::string name = text(entry, "section", "Settings");
        if (name.empty()) name = "Settings";
        if (!grouped.count(name)) {
            grouped[name] = json::array();
            order.push_back(name);
        }
        json settingRow =
            with(row("text." + text(entry, "key"), text(entry, "title"), "res://ic_mana_settings", kAstralRowLink), {{"detail", "Edit"}});
        if (!text(entry, "subtitle").empty()) settingRow["subtitle"] = text(entry, "subtitle");
        grouped[name].push_back(std::move(settingRow));
    }
    json sections = json::array();
    for (const auto &name : order) sections.push_back(section(titles.count(name) ? titles[name] : name, grouped[name]));
    sections.push_back(section("Manage", {with(row("reset", "Reset settings", "res://ic_mana_refresh", kAstralRowLink),
                                               {{"subtitle", "Return every option to its default"}})}));
    return page(text(plugin, "name"), sections);
}

json choiceRows(const json &items, const std::string &active, const std::string &prefix, const std::string &icon)
{
    json rows = json::array();
    for (const auto &item : items) {
        json choice =
            row(prefix + text(item, "id"), text(item, "name"), icon, text(item, "id") == active ? kAstralRowSelected : kAstralRowChoice);
        if (item.contains("description") && item["description"].is_string()) choice["subtitle"] = item["description"];
        rows.push_back(choice);
    }
    return rows;
}

json designPage()
{
    json themes = choiceRows(list(gPlatform.themes), call(gPlatform.activeTheme), "theme.", "res://ic_mana_paint_palette");
    json packRows = json::array();
    std::string activePack = call(gPlatform.activeIconPack);
    for (const auto &pack : list(gPlatform.iconPacks)) {
        json choice = row("icon-pack." + text(pack, "id"), text(pack, "name"), "res://ic_mana_image",
                          text(pack, "id") == activePack ? kAstralRowSelected : kAstralRowChoice);
        std::string description = text(pack, "description");
        long count = (long)number(pack, "count");
        choice["subtitle"] = count ? description + " · " + std::to_string(count) + " icons" : description;
        packRows.push_back(choice);
    }
    json emojiRows = choiceRows(list(gPlatform.emojiPacks), call(gPlatform.activeEmojiPack), "emoji-pack.", "res://ic_mana_reaction");
    emojiRows.push_back(with(
        row("emoji-animated", "Animated emoji", "res://ic_mana_sparkles", kAstralRowSwitch),
        {{"on", gPlatform.emojiAnimated && gPlatform.emojiAnimated()}, {"subtitle", "Use a pack's animated emoji where it has them"}}));
    return page("Design",
                {section("Themes", themes), section("Icon packs", packRows), section("Emoji packs", emojiRows),
                 section("How themes apply",
                         {with(row("theme-note", "Recolours Phoenix views right away", "res://ic_mana_circle_information", kAstralRowInfo),
                               {{"subtitle", "Colours change on screen as soon as you pick a theme"}})}),
                 section("Get more", {with(row("import-unbound-theme", "Import Unbound theme", "res://ic_mana_download", kAstralRowLink),
                                           {{"subtitle", "Convert an Unbound theme file into an astral theme"}}),
                                      row("browse", "Browse themes", "res://ic_mana_shop", kAstralRowLink)})});
}

std::optional<std::string> installedVersion(const std::string &identifier, const std::string &kind)
{
    if (kind == "theme") {
        for (const auto &theme : list(gPlatform.themes))
            if (text(theme, "id") == identifier) return text(theme, "version");
        return std::nullopt;
    }
    json plugin = pluginWithID(identifier);
    if (plugin.is_null()) return std::nullopt;
    return text(plugin, "version");
}

std::optional<std::string> installStatus(const std::string &identifier)
{
    return gPlatform.installStatus ? gPlatform.installStatus(identifier) : std::nullopt;
}

std::string packageIcon(const json &package, const std::string &identifier)
{
    json plugin = pluginWithID(identifier);
    if (!plugin.is_null()) return pluginIcon(plugin);
    if (!text(package, "icon").empty()) return manaAsset(text(package, "icon"));
    return "res://ic_mana_puzzle_piece_plus";
}

json packageRow(const json &package, const std::string &kind)
{
    std::string identifier = text(package, "id");
    auto installed = installedVersion(identifier, kind);
    auto status = installStatus(identifier);
    if (!status) status = !installed ? "Get" : astral::pages::versionIsNewer(text(package, "version"), *installed) ? "Update" : "Installed";
    std::string icon = kind == "theme" ? "res://ic_mana_paint_palette" : packageIcon(package, identifier);
    std::string subtitle = text(package, "description");
    if (package.contains("version") && package["version"].is_string()) subtitle = "v" + text(package, "version") + " · " + subtitle;
    return with(row(kind + "." + identifier, text(package, "name"), icon, kAstralRowLink), {{"detail", *status}, {"subtitle", subtitle}});
}

json catalogStatusRows()
{
    std::string state = call(gPlatform.catalogState);
    if (state == "loading") return {row("loading", "Loading the catalog…", "res://ic_mana_refresh", kAstralRowInfo)};
    if (state == "failed")
        return {with(row("failed", "Could not load the catalog", "res://ic_mana_bug", kAstralRowInfo),
                     {{"subtitle", call(gPlatform.catalogError)}})};
    return json::array();
}

json catalog()
{
    if (!gPlatform.catalog) return json::object();
    json value = gPlatform.catalog();
    return value.is_object() ? value : json::object();
}

json marketplacePage()
{
    json packages = catalog();
    json sections = json::array();
    json status = catalogStatusRows();
    if (!status.empty()) sections.push_back(section("Catalog", status));
    for (const std::string kind : {"plugin", "theme"}) {
        json rows = json::array();
        for (const auto &package : packages.value(kind + "s", json::array())) rows.push_back(packageRow(package, kind));
        if (!rows.empty()) sections.push_back(section(kind == "plugin" ? "Plugins" : "Themes", rows));
    }
    sections.push_back(section(
        "Source", {with(row("catalog-url", "Catalog", "res://ic_mana_link", kAstralRowInfo), {{"subtitle", call(gPlatform.catalogURL)}}),
                   row("refresh", "Refresh", "res://ic_mana_refresh", kAstralRowLink)}));
    return page("Marketplace", sections);
}

std::vector<std::pair<json, std::string>> pendingUpdates()
{
    std::vector<std::pair<json, std::string>> pending;
    json packages = catalog();
    for (const std::string kind : {"plugin", "theme"}) {
        for (const auto &package : packages.value(kind + "s", json::array())) {
            auto installed = installedVersion(text(package, "id"), kind);
            if (installed && astral::pages::versionIsNewer(text(package, "version"), *installed)) pending.push_back({package, kind});
        }
    }
    return pending;
}

constexpr int64_t kCatalogRefreshSeconds = 6 * 60 * 60;

bool autoUpdates()
{
    return !gPlatform.settings || gPlatform.settings->getBoolean("astral", "auto-update", true);
}

void toggleAutoUpdates()
{
    if (!gPlatform.settings) return;
    if (gPlatform.settings->set("astral", "auto-update", !autoUpdates())) gPlatform.settings->save();
}

std::string latestAstralVersion()
{
    json packages = catalog();
    return packages.contains("astral") && packages["astral"].is_object() ? text(packages["astral"], "version") : "";
}

void noticeAstralUpdate()
{
    std::string latest = latestAstralVersion();
    if (latest.empty() || !astral::pages::versionIsNewer(latest, gPlatform.version) || !gPlatform.settings) return;
    if (gPlatform.settings->getString("astral", "update-notice", "") == latest) return;
    if (gPlatform.settings->set("astral", "update-notice", latest)) gPlatform.settings->save();
    if (gPlatform.notify) gPlatform.notify("astral " + latest + " is available", "See Settings, astral, Updates");
}

void installPendingUpdates(const std::function<void()> &changed)
{
    if (!gPlatform.installPackage) return;
    for (const auto &[package, kind] : pendingUpdates()) gPlatform.installPackage(package, kind, changed);
}

void catalogRefreshed(const std::function<void()> &changed)
{
    if (autoUpdates()) installPendingUpdates(changed);
    noticeAstralUpdate();
    changed();
}

int64_t lastCatalogRefresh()
{
    auto value = gPlatform.settings ? gPlatform.settings->get("astral", "catalog-refreshed-at") : std::nullopt;
    return value && value->is_number_integer() ? value->get<int64_t>() : 0;
}

json updatesPage(const astral::SettingsPageContext &context)
{
    std::string latest = latestAstralVersion();
    std::string astralStatus = latest.empty()                                             ? "Unknown"
                               : astral::pages::versionIsNewer(latest, gPlatform.version) ? "Update to " + latest
                                                                                          : "Up to date";
    json addonRows = json::array();
    for (const auto &[package, kind] : pendingUpdates()) {
        std::string identifier = text(package, "id");
        auto status = installStatus(identifier);
        addonRows.push_back(with(row(kind + "." + identifier, text(package, "name"), "res://ic_mana_cloud_download", kAstralRowLink),
                                 {{"detail", status ? *status : "v" + text(package, "version")},
                                  {"subtitle", "Installed v" + installedVersion(identifier, kind).value_or("")}}));
    }
    if (addonRows.size() > 1) addonRows.push_back(row("update-all", "Update all", "res://ic_mana_download", kAstralRowLink));
    if (addonRows.empty()) addonRows.push_back(row("current", "Everything is up to date", "res://ic_mana_circle_check", kAstralRowInfo));
    json sections = json::array();
    json status = catalogStatusRows();
    if (!status.empty()) sections.push_back(section("Catalog", status));
    sections.push_back(section("astral", {with(row("astral", "astral", "res://ic_mana_settings", kAstralRowInfo),
                                               {{"detail", astralStatus}, {"subtitle", "Installed " + gPlatform.version}})}));
    sections.push_back(section("Discord", {with(row("discord-build", "Build", "res://ic_mana_mobile_phone_settings", kAstralRowInfo),
                                                {{"detail", context.compatibility == "ready" ? "Supported" : "Unsupported"},
                                                 {"subtitle", context.discordVersion}})}));
    sections.push_back(section("Addons", addonRows));
    sections.push_back(section("Check", {row("refresh", "Check for updates", "res://ic_mana_refresh", kAstralRowLink),
                                         with(row("auto-update", "Update automatically", "res://ic_mana_download", kAstralRowSwitch),
                                              {{"on", autoUpdates()}, {"subtitle", "Install addon updates when Discord opens"}})}));
    return page("Updates", sections);
}

json catalogPackage(const std::string &kind, const std::string &identifier)
{
    for (const auto &package : catalog().value(kind + "s", json::array()))
        if (text(package, "id") == identifier) return package;
    return nullptr;
}

void installFromKey(const std::string &key, const std::function<void()> &changed)
{
    size_t dot = key.find('.');
    if (dot == std::string::npos || !gPlatform.installPackage) return;
    std::string kind = key.substr(0, dot), identifier = key.substr(dot + 1);
    json package = catalogPackage(kind, identifier);
    auto installed = installedVersion(identifier, kind);
    if (!package.is_null() && (!installed || astral::pages::versionIsNewer(text(package, "version"), *installed)))
        gPlatform.installPackage(package, kind, changed);
}

void toggleEnabled(const std::string &identifier, const json &plugin)
{
    astral::luahost::setEnabled(identifier, flag(plugin, "failed") || !flag(plugin, "enabled"));
}

std::optional<std::string> pluginAction(const std::string &identifier, const std::string &key, const std::function<void()> &changed)
{
    json plugin = pluginWithID(identifier);
    if (plugin.is_null()) return std::string("..");
    if (key == "switch.enabled" || key == "retry") {
        toggleEnabled(identifier, plugin);
    } else if (key.rfind("switch.setting.", 0) == 0) {
        std::string settingKey = key.substr(15);
        for (const auto &entry : plugin.value("switches", json::array()))
            if (text(entry, "key") == settingKey) astral::luahost::setSwitch(identifier, settingKey, !flag(entry, "on"));
    } else if (key.rfind("number.", 0) == 0) {
        std::string rest = key.substr(7);
        size_t equals = rest.rfind('=');
        if (equals != std::string::npos) {
            std::string settingKey = rest.substr(0, equals);
            int position = atoi(rest.substr(equals + 1).c_str());
            for (const auto &entry : plugin.value("switches", json::array())) {
                if (!flag(entry, "number") || text(entry, "key") != settingKey) continue;
                astral::luahost::setNumber(identifier, settingKey, number(entry, "min") + position * number(entry, "step"));
            }
        }
    } else if (key.rfind("text.", 0) == 0 && gPlatform.editText) {
        std::string settingKey = key.substr(5);
        for (const auto &entry : plugin.value("textSettings", json::array())) {
            if (text(entry, "key") != settingKey) continue;
            std::string title = text(entry, "title");
            std::string value = text(entry, "value");
            size_t maximumLength = static_cast<size_t>(std::max(0.0, number(entry, "maxLength")));
            gPlatform.editText(title, value, maximumLength, [identifier, settingKey, changed](std::optional<std::string> edited) {
                if (edited) astral::luahost::setText(identifier, settingKey, *edited);
                if (changed) changed();
            });
            break;
        }
    } else if (key.rfind("choice.", 0) == 0) {
        std::string rest = key.substr(7);
        size_t dot = rest.find('.');
        if (dot != std::string::npos) astral::luahost::setChoice(identifier, rest.substr(0, dot), rest.substr(dot + 1));
    } else if (key == "reset") {
        astral::luahost::resetSettings(identifier);
    } else if (key == "uninstall" && !flag(plugin, "bundled")) {
        std::error_code error;
        std::filesystem::remove_all(text(plugin, "directory"), error);
        astral::luahost::removeStorage(identifier);
        astral::luahost::remove(identifier);
        astral::luahost::rescan();
        changed();
        return std::string("..");
    }
    return std::nullopt;
}

} // namespace

namespace astral::pages {

bool catalogRefreshDue(int64_t now, int64_t last)
{
    return last <= 0 || now < last || now - last >= kCatalogRefreshSeconds;
}

void refreshCatalogIfDue(int64_t now, bool force, const std::function<void()> &changed)
{
    if (!gPlatform.refreshCatalog || (!force && !catalogRefreshDue(now, lastCatalogRefresh()))) return;
    if (gPlatform.settings && gPlatform.settings->set("astral", "catalog-refreshed-at", now)) gPlatform.settings->save();
    gPlatform.refreshCatalog([changed] { catalogRefreshed(changed); });
}

void configure(SettingsPagesPlatform platform)
{
    gPlatform = std::move(platform);
}

json category()
{
    return {{"title", "astral"},
            {"rows",
             {{{"key", "general"}, {"title", "astral"}, {"icon", "res://astral_galaxy"}},
              {{"key", "plugins"}, {"title", "Plugins"}, {"icon", "res://ic_mana_puzzle_piece"}},
              {{"key", "design"}, {"title", "Design"}, {"icon", "res://ic_mana_paintbrush_thick"}},
              {{"key", "marketplace"}, {"title", "Marketplace"}, {"icon", "res://ic_mana_shop"}},
              {{"key", "updates"}, {"title", "Updates"}, {"icon", "res://ic_mana_download"}}}}};
}

bool versionIsNewer(const std::string &candidate, const std::string &installed)
{
    if (candidate.empty()) return false;
    if (installed.empty()) return true;
    size_t left = 0, right = 0;
    while (left < candidate.size() || right < installed.size()) {
        if (left < candidate.size() && right < installed.size() && std::isdigit((unsigned char)candidate[left]) &&
            std::isdigit((unsigned char)installed[right])) {
            size_t leftEnd = left, rightEnd = right;
            while (leftEnd < candidate.size() && std::isdigit((unsigned char)candidate[leftEnd])) leftEnd++;
            while (rightEnd < installed.size() && std::isdigit((unsigned char)installed[rightEnd])) rightEnd++;
            unsigned long long a = strtoull(candidate.substr(left, leftEnd - left).c_str(), nullptr, 10);
            unsigned long long b = strtoull(installed.substr(right, rightEnd - right).c_str(), nullptr, 10);
            if (a != b) return a > b;
            left = leftEnd;
            right = rightEnd;
            continue;
        }
        if (left >= candidate.size()) return false;
        if (right >= installed.size()) return true;
        if (candidate[left] != installed[right]) return candidate[left] > installed[right];
        left++;
        right++;
    }
    return false;
}

json model(const std::string &route)
{
    SettingsPageContext context = gPlatform.context ? gPlatform.context() : SettingsPageContext{};
    if (route == "general") return generalPage(context);
    if (route == "plugins" || route.empty()) return pluginsPage();
    if (route.rfind(kPluginSettingsRoute, 0) == 0) return pluginSettingsPage(route.substr(kPluginSettingsRoute.size()));
    if (route.rfind(kPluginRoute, 0) == 0) return pluginPage(route.substr(kPluginRoute.size()));
    if (route == "design") return designPage();
    if (route == "marketplace") return marketplacePage();
    if (route == "updates") return updatesPage(context);
    return page("astral", json::array());
}

std::optional<std::string> action(const std::string &route, const std::string &key, const std::function<void()> &changed)
{
    if (route.rfind(kPluginSettingsRoute, 0) == 0) return pluginAction(route.substr(kPluginSettingsRoute.size()), key, changed);
    if (route.rfind(kPluginRoute, 0) == 0) return pluginAction(route.substr(kPluginRoute.size()), key, changed);
    if (key == "browse") return std::string("marketplace");
    if (key.rfind("switch.plugin.", 0) == 0) {
        std::string identifier = key.substr(14);
        json plugin = pluginWithID(identifier);
        if (!plugin.is_null()) toggleEnabled(identifier, plugin);
        return std::nullopt;
    }
    if (key.rfind("settings.plugin.", 0) == 0) return kPluginSettingsRoute + key.substr(16);
    if (key.rfind("more.plugin.", 0) == 0) return kPluginRoute + key.substr(12);
    if (key.rfind("filter.", 0) == 0) {
        std::lock_guard<std::mutex> lock(gFilterMutex);
        gPluginFilter = key.substr(7);
        return std::nullopt;
    }
    if (key == "import-unbound-theme") {
        if (gPlatform.importUnboundTheme) gPlatform.importUnboundTheme(changed);
        return std::nullopt;
    }
    if (key == "export-backup") {
        if (gPlatform.exportBackup) gPlatform.exportBackup();
        return std::nullopt;
    }
    if (key == "import-backup") {
        if (gPlatform.importBackup) gPlatform.importBackup(changed);
        return std::nullopt;
    }
    if (key.rfind("report.", 0) == 0) {
        if (gPlatform.openReport) gPlatform.openReport((size_t)strtoull(key.substr(7).c_str(), nullptr, 10));
        return std::nullopt;
    }
    if (key == "switch.developer-mode") {
        toggle("developer-mode");
        for (const auto &plugin : luahost::plugins())
            if (flag(plugin, "enabled") && !flag(plugin, "signed")) luahost::setEnabled(text(plugin, "id"), true);
        return std::nullopt;
    }
    if (key == "switch.auto-update") {
        toggleAutoUpdates();
        return std::nullopt;
    }
    if (key == "switch.staff-devtools") {
        toggle("staff-devtools");
        if (gPlatform.restartGatewaySession) gPlatform.restartGatewaySession();
        return std::nullopt;
    }
    if (key == "switch.safe-mode") {
        toggle("safe-mode");
        return std::nullopt;
    }
    if (key == "soft-restart") {
        if (gPlatform.softRestart) gPlatform.softRestart();
        return std::nullopt;
    }
    if (key == "reload") {
        for (const auto &plugin : luahost::plugins())
            if (flag(plugin, "enabled")) luahost::setEnabled(text(plugin, "id"), true);
        return std::nullopt;
    }
    if (route == "design" && key.rfind("emoji-pack.", 0) == 0) {
        if (gPlatform.selectEmojiPack) gPlatform.selectEmojiPack(key.substr(11));
        return std::nullopt;
    }
    if (route == "design" && key == "switch.emoji-animated") {
        if (gPlatform.setEmojiAnimated) gPlatform.setEmojiAnimated(!(gPlatform.emojiAnimated && gPlatform.emojiAnimated()));
        return std::nullopt;
    }
    if (route == "design" && key.rfind("icon-pack.", 0) == 0) {
        if (gPlatform.selectIconPack) gPlatform.selectIconPack(key.substr(10));
        return std::nullopt;
    }
    if (route == "design" && key.rfind("theme.", 0) == 0) {
        if (gPlatform.applyTheme) gPlatform.applyTheme(key.substr(6));
        return std::nullopt;
    }
    if (key == "refresh") {
        refreshCatalogIfDue(static_cast<int64_t>(std::time(nullptr)), true, changed);
        return std::nullopt;
    }
    if (key == "update-all") {
        installPendingUpdates(changed);
        return std::nullopt;
    }
    if (route == "marketplace" || route == "updates") installFromKey(key, changed);
    return std::nullopt;
}

void withSpec(const json &model, const std::function<void(const AstralPageSpec *page)> &body)
{
    if (!model.is_object()) return;
    json sections = objects(model.value("sections", json::array()));
    std::vector<std::vector<AstralRowSpec>> rows(sections.size());
    std::vector<json> sectionRows(sections.size());
    std::vector<AstralSectionSpec> specs;
    std::vector<std::string> sectionTitles;
    sectionTitles.reserve(sections.size());
    auto optional = [](const json &object, const char *key) -> const char * {
        auto field = object.find(key);
        return field != object.end() && field->is_string() && !field->get_ref<const std::string &>().empty()
                   ? field->get_ref<const std::string &>().c_str()
                   : nullptr;
    };
    for (size_t index = 0; index < sections.size(); index++) {
        sectionRows[index] = objects(sections[index].value("rows", json::array()));
        for (const auto &entry : sectionRows[index]) {
            const char *key = optional(entry, "key"), *title = optional(entry, "title");
            rows[index].push_back({key ? key : "", title ? title : "", optional(entry, "subtitle"), optional(entry, "detail"),
                                   optional(entry, "icon"), (AstralRowKind)entry.value("kind", 0), entry.value("on", false) ? 1 : 0,
                                   entry.value("steps", 0), entry.value("position", 0)});
        }
        sectionTitles.push_back(text(sections[index], "title"));
    }
    for (size_t index = 0; index < sections.size(); index++)
        specs.push_back({sectionTitles[index].c_str(), rows[index].data(), rows[index].size()});
    json toolbarModel = model.contains("toolbar") && model["toolbar"].is_object() ? model["toolbar"] : json::object();
    json chipList = objects(toolbarModel.value("chips", json::array()));
    std::vector<AstralChipSpec> chips;
    for (const auto &chip : chipList) {
        const char *key = optional(chip, "key"), *title = optional(chip, "title");
        chips.push_back({key ? key : "", title ? title : "", chip.value("selected", false) ? 1 : 0});
    }
    AstralToolbarSpec toolbar = {chips.data(), chips.size(), optional(toolbarModel, "buttonKey"), optional(toolbarModel, "buttonIcon")};
    json cardList = objects(model.value("cards", json::array()));
    std::vector<AstralCardSpec> cards;
    for (const auto &card : cardList) {
        AstralCardSpec spec = {};
        const char *key = optional(card, "key"), *title = optional(card, "title");
        spec.key = key ? key : "";
        spec.title = title ? title : "";
        spec.authors = optional(card, "authors");
        spec.description = optional(card, "description");
        spec.version = optional(card, "version");
        spec.icon = optional(card, "icon");
        spec.error = optional(card, "error");
        if (card.contains("avatars") && card["avatars"].is_array()) {
            for (const auto &avatar : card["avatars"]) {
                if (spec.avatarCount < 3 && avatar.is_string())
                    spec.avatars[spec.avatarCount++] = avatar.get_ref<const std::string &>().c_str();
            }
        }
        spec.hasSettings = card.value("hasSettings", false);
        spec.on = card.value("on", false);
        spec.failed = card.value("failed", false);
        cards.push_back(spec);
    }
    std::string title = text(model, "title");
    AstralPageSpec spec = {title.c_str(), specs.data(), specs.size(), toolbarModel.empty() ? nullptr : &toolbar,
                           cards.data(),  cards.size()};
    body(&spec);
}

} // namespace astral::pages
