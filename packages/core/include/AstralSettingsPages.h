#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

#include "AstralDevServer.h"
#include "AstralLuaHost.h"
#include "AstralRust.h"
#include "AstralSettings.h"
#include "PhoenixWire.h"

extern "C" {
typedef AstralRustString (*AstralPagesHostCall)(const char *method, size_t methodLength, const char *arguments, size_t argumentsLength,
                                                AstralCallback callback);
void astral_settings_pages_configure(AstralPagesHostCall host, const AstralSettingsHandle *settings, const char *version,
                                     size_t versionLength);
AstralRustString astral_settings_pages_category(void);
AstralRustString astral_settings_pages_model(const char *route, size_t length);
AstralRustString astral_settings_pages_action(const char *route, size_t routeLength, const char *key, size_t keyLength,
                                              AstralCallback changed);
int astral_settings_pages_version_is_newer(const char *candidate, size_t candidateLength, const char *installed, size_t installedLength);
int astral_settings_pages_catalog_refresh_due(int64_t now, int64_t last);
void astral_settings_pages_refresh_catalog_if_due(int64_t now, int force, AstralCallback changed);
void astral_settings_pages_with_spec(const char *model, size_t length, void (*body)(void *context, const AstralPageSpec *spec),
                                     void *context);
}

namespace astral {

struct SettingsPageContext {
    bool recovering = false;
    std::string discordVersion;
    std::string compatibility;
};

struct SettingsPagesPlatform {
    SettingsStore *settings = nullptr;
    std::string version;
    std::function<SettingsPageContext()> context;
    std::function<nlohmann::json()> crashReports;
    std::function<void()> restartGatewaySession;
    std::function<void(size_t index)> openReport;
    std::function<uint64_t()> blockedRequests;
    std::function<nlohmann::json()> themes;
    std::function<std::string()> activeTheme;
    std::function<void(const std::string &identifier)> applyTheme;
    std::function<nlohmann::json()> iconPacks;
    std::function<std::string()> activeIconPack;
    std::function<void(const std::string &identifier)> selectIconPack;
    std::function<nlohmann::json()> emojiPacks;
    std::function<std::string()> activeEmojiPack;
    std::function<void(const std::string &identifier)> selectEmojiPack;
    std::function<bool()> emojiAnimated;
    std::function<void(bool animated)> setEmojiAnimated;
    std::function<nlohmann::json()> catalog;
    std::function<std::string()> catalogState;
    std::function<std::string()> catalogError;
    std::function<std::string()> catalogURL;
    std::function<std::optional<std::string>(const std::string &identifier)> installStatus;
    std::function<void(const nlohmann::json &package, const std::string &kind, const std::function<void()> &changed)> installPackage;
    std::function<void(const std::function<void()> &changed)> refreshCatalog;
    std::function<std::string(const std::string &userID, size_t index)> authorAvatar;
    std::function<std::optional<std::string>(const nlohmann::json &plugin)> pluginIconSVG;
    std::function<void(const std::string &title, const std::string &value, size_t maximumLength,
                       const std::function<void(std::optional<std::string>)> &completion)>
        editText;
    std::function<void(const std::function<void()> &changed)> importUnboundTheme;
    std::function<void()> exportBackup;
    std::function<void(const std::function<void()> &changed)> importBackup;
    std::function<void()> softRestart;
    std::function<void(const std::string &title, const std::string &message)> notify;
};

namespace pages {

inline SettingsPagesPlatform &platform()
{
    static SettingsPagesPlatform stored;
    return stored;
}

inline nlohmann::json parsedJSON(AstralRustString string)
{
    return nlohmann::json::parse(rust::take(string), nullptr, false);
}

inline std::string argument(const nlohmann::json &arguments, size_t index)
{
    return arguments.is_array() && index < arguments.size() && arguments[index].is_string() ? arguments[index].get<std::string>() : "";
}

inline nlohmann::json at(const nlohmann::json &arguments, size_t index)
{
    return arguments.is_array() && index < arguments.size() ? arguments[index] : nlohmann::json();
}

inline nlohmann::json optionalText(const std::optional<std::string> &value)
{
    return value ? nlohmann::json(*value) : nlohmann::json();
}

template <typename Source> nlohmann::json produced(const Source &source)
{
    return source ? nlohmann::json(source()) : nlohmann::json();
}

inline nlohmann::json dispatch(const std::string &method, const nlohmann::json &arguments, AstralCallback callback)
{
    auto &host = platform();
    auto reply = rust::adoptReply(callback);
    std::function<void()> changed = [reply] {
        if (reply) reply(std::nullopt);
    };
    if (method == "capabilities")
        return {{"restartGatewaySession", static_cast<bool>(host.restartGatewaySession)},
                {"editText", static_cast<bool>(host.editText)},
                {"installPackage", static_cast<bool>(host.installPackage)},
                {"refreshCatalog", static_cast<bool>(host.refreshCatalog)}};
    if (method == "context") {
        SettingsPageContext context = host.context ? host.context() : SettingsPageContext{};
        return {{"recovering", context.recovering}, {"discordVersion", context.discordVersion}, {"compatibility", context.compatibility}};
    }
    if (method == "plugins") return luahost::plugins();
    if (method == "setEnabled") luahost::setEnabled(argument(arguments, 0), at(arguments, 1).is_boolean() && at(arguments, 1).get<bool>());
    if (method == "setSwitch")
        luahost::setSwitch(argument(arguments, 0), argument(arguments, 1), at(arguments, 2).is_boolean() && at(arguments, 2).get<bool>());
    if (method == "setChoice") luahost::setChoice(argument(arguments, 0), argument(arguments, 1), argument(arguments, 2));
    if (method == "setNumber")
        luahost::setNumber(argument(arguments, 0), argument(arguments, 1),
                           at(arguments, 2).is_number() ? at(arguments, 2).get<double>() : 0);
    if (method == "setText") luahost::setText(argument(arguments, 0), argument(arguments, 1), argument(arguments, 2));
    if (method == "pressSettingsButton") luahost::pressSettingsButton(argument(arguments, 0), argument(arguments, 1));
    if (method == "resetSettings") luahost::resetSettings(argument(arguments, 0));
    if (method == "removeStorage") luahost::removeStorage(argument(arguments, 0));
    if (method == "remove") luahost::remove(argument(arguments, 0));
    if (method == "rescan") luahost::rescan();
    if (method == "pairingCode") return dev::pairingCode();
    if (method == "crashReports") return produced(host.crashReports);
    if (method == "blockedRequests") return produced(host.blockedRequests);
    if (method == "themes") return produced(host.themes);
    if (method == "activeTheme") return produced(host.activeTheme);
    if (method == "iconPacks") return produced(host.iconPacks);
    if (method == "activeIconPack") return produced(host.activeIconPack);
    if (method == "emojiPacks") return produced(host.emojiPacks);
    if (method == "activeEmojiPack") return produced(host.activeEmojiPack);
    if (method == "emojiAnimated") return produced(host.emojiAnimated);
    if (method == "catalog") return produced(host.catalog);
    if (method == "catalogState") return produced(host.catalogState);
    if (method == "catalogError") return produced(host.catalogError);
    if (method == "catalogURL") return produced(host.catalogURL);
    if (method == "applyTheme" && host.applyTheme) host.applyTheme(argument(arguments, 0));
    if (method == "selectIconPack" && host.selectIconPack) host.selectIconPack(argument(arguments, 0));
    if (method == "selectEmojiPack" && host.selectEmojiPack) host.selectEmojiPack(argument(arguments, 0));
    if (method == "setEmojiAnimated" && host.setEmojiAnimated)
        host.setEmojiAnimated(at(arguments, 0).is_boolean() && at(arguments, 0).get<bool>());
    if (method == "installStatus") return host.installStatus ? optionalText(host.installStatus(argument(arguments, 0))) : nlohmann::json();
    if (method == "installPackage" && host.installPackage) host.installPackage(at(arguments, 0), argument(arguments, 1), changed);
    if (method == "refreshCatalog" && host.refreshCatalog) host.refreshCatalog(changed);
    if (method == "authorAvatar" && host.authorAvatar)
        return host.authorAvatar(argument(arguments, 0), at(arguments, 1).is_number_unsigned() ? at(arguments, 1).get<size_t>() : 0);
    if (method == "pluginIconSVG") return host.pluginIconSVG ? optionalText(host.pluginIconSVG(at(arguments, 0))) : nlohmann::json();
    if (method == "editText" && host.editText) {
        size_t maximum = at(arguments, 2).is_number_unsigned() ? at(arguments, 2).get<size_t>() : 0;
        host.editText(argument(arguments, 0), argument(arguments, 1), maximum, reply);
        return nullptr;
    }
    if (method == "importUnboundTheme" && host.importUnboundTheme) host.importUnboundTheme(changed);
    if (method == "exportBackup" && host.exportBackup) host.exportBackup();
    if (method == "importBackup" && host.importBackup) host.importBackup(changed);
    if (method == "softRestart" && host.softRestart) host.softRestart();
    if (method == "restartGatewaySession" && host.restartGatewaySession) host.restartGatewaySession();
    if (method == "openReport" && host.openReport)
        host.openReport(at(arguments, 0).is_number_unsigned() ? at(arguments, 0).get<size_t>() : 0);
    if (method == "notify" && host.notify) host.notify(argument(arguments, 0), argument(arguments, 1));
    return nullptr;
}

inline AstralRustString hostCall(const char *method, size_t methodLength, const char *arguments, size_t argumentsLength,
                                 AstralCallback callback)
{
    auto parsed = nlohmann::json::parse(std::string(arguments ? arguments : "", argumentsLength), nullptr, false);
    auto result = dispatch(std::string(method ? method : "", methodLength), parsed, callback);
    if (result.is_null()) return {nullptr, 0};
    return rust::give(result.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace));
}

inline void configure(SettingsPagesPlatform configured)
{
    platform() = std::move(configured);
    const auto &version = platform().version;
    astral_settings_pages_configure(&hostCall, platform().settings ? platform().settings->handle() : nullptr, version.data(),
                                    version.size());
}

inline nlohmann::json category()
{
    return parsedJSON(astral_settings_pages_category());
}

inline nlohmann::json model(const std::string &route)
{
    return parsedJSON(astral_settings_pages_model(route.data(), route.size()));
}

inline std::optional<std::string> action(const std::string &route, const std::string &key, const std::function<void()> &changed)
{
    return rust::takeIfPresent(
        astral_settings_pages_action(route.data(), route.size(), key.data(), key.size(), rust::changedCallback(changed)));
}

inline bool versionIsNewer(const std::string &candidate, const std::string &installed)
{
    return astral_settings_pages_version_is_newer(candidate.data(), candidate.size(), installed.data(), installed.size());
}

inline bool catalogRefreshDue(int64_t now, int64_t last)
{
    return astral_settings_pages_catalog_refresh_due(now, last);
}

inline void refreshCatalogIfDue(int64_t now, bool force, const std::function<void()> &changed)
{
    astral_settings_pages_refresh_catalog_if_due(now, force, rust::changedCallback(changed));
}

inline void withSpec(const nlohmann::json &model, const std::function<void(const AstralPageSpec *page)> &body)
{
    std::string text = model.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    auto run = [](void *context, const AstralPageSpec *spec) {
        (*static_cast<const std::function<void(const AstralPageSpec *)> *>(context))(spec);
    };
    astral_settings_pages_with_spec(text.data(), text.size(), run, const_cast<std::function<void(const AstralPageSpec *)> *>(&body));
}

} // namespace pages

} // namespace astral
