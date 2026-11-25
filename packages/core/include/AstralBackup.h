#pragma once

#include <functional>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>

#include "AstralMarketplace.h"
#include "AstralRust.h"
#include "AstralSettings.h"

extern "C" {
AstralRustString astral_backup_export(const AstralSettingsHandle *settings, const char *version, size_t versionLength, const char *plugins,
                                      size_t pluginsLength, const char *themes, size_t themesLength);
int astral_backup_import(const AstralSettingsHandle *settings, const char *contents, size_t contentsLength, const char *plugins,
                         size_t pluginsLength, const char *themes, size_t themesLength, AstralRustString *result);
}

namespace astral::backup {
inline std::string exportTOML(const SettingsStore &settings, const std::string &version, const nlohmann::json &plugins,
                              const nlohmann::json &themes)
{
    std::string pluginText = plugins.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    std::string themeText = themes.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    return rust::take(astral_backup_export(settings.handle(), version.data(), version.size(), pluginText.data(), pluginText.size(),
                                           themeText.data(), themeText.size()));
}

inline std::optional<std::string> import(const std::string &contents, const SettingsStore &settings, const nlohmann::json &plugins,
                                         const nlohmann::json &themes, const std::function<void()> &changed)
{
    std::string pluginText = plugins.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    std::string themeText = themes.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    AstralRustString result{};
    bool imported = astral_backup_import(settings.handle(), contents.data(), contents.size(), pluginText.data(), pluginText.size(),
                                         themeText.data(), themeText.size(), &result);
    std::string text = rust::take(result);
    if (!imported) return text;
    nlohmann::json installs = nlohmann::json::parse(text, nullptr, false);
    for (const auto &install : installs.is_array() ? installs : nlohmann::json::array())
        marketplace::install(install["package"], install.value("kind", ""), changed);
    return std::nullopt;
}
} // namespace astral::backup
