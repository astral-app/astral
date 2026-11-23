#pragma once

#include <functional>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace astral {
class SettingsStore;
class Themes;
}

namespace astral::android {
class Marketplace;

std::string exportBackup(SettingsStore &settings, const std::string &version, const nlohmann::json &plugins, const nlohmann::json &themes);
std::optional<std::string> importBackup(const std::string &contents, SettingsStore &settings, Themes &themes, Marketplace &marketplace,
                                        const std::function<void(const std::string &)> &applyTheme, const std::function<void()> &changed);
}
