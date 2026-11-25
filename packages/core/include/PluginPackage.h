#pragma once

#include <cstddef>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "AstralRust.h"
#include "LuaLocale.h"

extern "C" {
AstralRustString astral_plugin_package_load(const char *directory, size_t directoryLength, const char *preferred, size_t preferredLength,
                                            size_t sourceLimit, size_t modulesLimit);
}

namespace astral {
struct PackageLimits {
    size_t source = 256u << 10;
    size_t modules = 1u << 20;
};

struct PluginPackage {
    std::string identifier;
    std::string name;
    std::string version;
    std::string description;
    std::string icon;
    std::string iconSvg;
    std::string main;
    std::string directory;
    std::string source;
    std::vector<std::pair<std::string, std::string>> authors;
    std::set<std::string> permissions;
    bool enabledByDefault = false;
    std::map<std::string, std::string> modules;
    std::string mainBytecode;
    std::map<std::string, std::string> moduleBytecode;
    std::map<std::string, std::string> strings;
    std::map<std::string, std::string> fallbackStrings;
    std::string locale;

    std::string translate(const std::string &key, const std::vector<std::pair<std::string, std::string>> &values) const
    {
        auto found = strings.find(key);
        if (found != strings.end()) return lua::fillPlaceholders(found->second, values);
        auto fallback = fallbackStrings.find(key);
        return lua::fillPlaceholders(fallback != fallbackStrings.end() ? fallback->second : key, values);
    }
};

inline std::map<std::string, std::string> stringMap(const nlohmann::json &object)
{
    std::map<std::string, std::string> map;
    if (!object.is_object()) return map;
    for (const auto &[key, value] : object.items())
        if (value.is_string()) map[key] = value.get<std::string>();
    return map;
}

inline std::optional<PluginPackage> loadPluginPackage(const std::string &directory, const std::vector<std::string> &preferredLanguages,
                                                      PackageLimits limits = {})
{
    std::string preferred = lua::joinedList(preferredLanguages);
    AstralRustString loaded =
        astral_plugin_package_load(directory.data(), directory.size(), preferred.data(), preferred.size(), limits.source, limits.modules);
    if (!loaded.bytes) return std::nullopt;
    std::vector<std::string> frames = rust::takeFrames(loaded);
    if (frames.size() < 3 || (frames.size() - 3) % 3 != 0) return std::nullopt;
    auto header = nlohmann::json::parse(frames[0], nullptr, false);
    if (!header.is_object()) return std::nullopt;
    PluginPackage package;
    package.identifier = header.value("identifier", "");
    package.name = header.value("name", "");
    package.version = header.value("version", "");
    package.description = header.value("description", "");
    package.icon = header.value("icon", "");
    package.iconSvg = header.value("iconSvg", "");
    package.main = header.value("main", "");
    package.directory = header.value("directory", "");
    package.enabledByDefault = header.value("enabledByDefault", false);
    package.locale = header.value("locale", "");
    package.strings = stringMap(header.value("strings", nlohmann::json::object()));
    package.fallbackStrings = stringMap(header.value("fallbackStrings", nlohmann::json::object()));
    for (const auto &author : header.value("authors", nlohmann::json::array()))
        if (author.is_array() && author.size() == 2 && author[0].is_string() && author[1].is_string())
            package.authors.push_back({author[0].get<std::string>(), author[1].get<std::string>()});
    for (const auto &permission : header.value("permissions", nlohmann::json::array()))
        if (permission.is_string()) package.permissions.insert(permission.get<std::string>());
    package.source = std::move(frames[1]);
    package.mainBytecode = std::move(frames[2]);
    for (size_t index = 3; index + 2 < frames.size(); index += 3) {
        package.modules[frames[index]] = std::move(frames[index + 1]);
        if (!frames[index + 2].empty()) package.moduleBytecode[frames[index]] = std::move(frames[index + 2]);
    }
    return package;
}

struct PluginHealth {
    int errors = 0;
    bool failed = false;
    std::string lastError;

    void reset()
    {
        errors = 0;
        failed = false;
        lastError.clear();
    }

    bool markFailed()
    {
        if (failed) return false;
        failed = true;
        return true;
    }

    bool recordError(const std::string &stage, const std::string &message, int maximumErrors)
    {
        lastError = stage + ": " + message;
        return ++errors >= maximumErrors && markFailed();
    }
};
} // namespace astral
