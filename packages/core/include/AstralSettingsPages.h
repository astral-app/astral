#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

#include "PhoenixWire.h"

namespace astral {

class SettingsStore;

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

void configure(SettingsPagesPlatform platform);
nlohmann::json category();
nlohmann::json model(const std::string &route);
std::optional<std::string> action(const std::string &route, const std::string &key, const std::function<void()> &changed);
bool versionIsNewer(const std::string &candidate, const std::string &installed);
bool catalogRefreshDue(int64_t now, int64_t last);
void refreshCatalogIfDue(int64_t now, bool force, const std::function<void()> &changed);
void withSpec(const nlohmann::json &model, const std::function<void(const AstralPageSpec *page)> &body);

} // namespace pages

} // namespace astral
