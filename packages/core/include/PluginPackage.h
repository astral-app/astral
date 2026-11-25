#pragma once

#include <cstddef>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

namespace astral {
struct PackageLimits {
    size_t source = 256u << 10;
    size_t modules = 1u << 20;
    size_t nameLength = 48;
    size_t iconSvg = 16384;
    size_t authors = 12;
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

    std::string translate(const std::string &key, const std::vector<std::pair<std::string, std::string>> &values) const;
};

std::optional<PluginPackage> loadPluginPackage(const std::string &directory, const std::vector<std::string> &preferredLanguages,
                                               PackageLimits limits = {});

struct PluginHealth {
    int errors = 0;
    bool failed = false;
    std::string lastError;

    void reset();
    bool recordError(const std::string &stage, const std::string &message, int maximumErrors);
    bool markFailed();
};
} // namespace astral
