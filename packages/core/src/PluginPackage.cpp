#define TOML_EXCEPTIONS 0

#include "PluginPackage.h"

#include <filesystem>
#include <fstream>
#include <sstream>
#include <toml++/toml.hpp>

#include "LuaLocale.h"

namespace astral {
namespace {
namespace fs = std::filesystem;

std::optional<std::string> readFile(const fs::path &path, size_t limit)
{
    std::error_code error;
    auto size = fs::file_size(path, error);
    if (error || size > limit) return std::nullopt;
    std::ifstream stream(path, std::ios::binary);
    if (!stream) return std::nullopt;
    std::ostringstream contents;
    contents << stream.rdbuf();
    return contents.str();
}

std::optional<toml::table> readTable(const fs::path &path)
{
    auto text = readFile(path, 1u << 20);
    if (!text) return std::nullopt;
    toml::parse_result result = toml::parse(*text);
    if (!result) return std::nullopt;
    return std::move(result).table();
}

std::string stringAt(const toml::table &table, const char *key, const std::string &fallback = "")
{
    auto value = table[key].value<std::string>();
    return value ? *value : fallback;
}

std::string truncateCodepoints(const std::string &text, size_t limit)
{
    size_t codepoints = 0;
    for (size_t index = 0; index < text.size(); index++) {
        if ((static_cast<unsigned char>(text[index]) & 0xC0) != 0x80 && codepoints++ == limit) return text.substr(0, index);
    }
    return text;
}

std::map<std::string, std::string> loadStrings(const fs::path &path)
{
    std::map<std::string, std::string> strings;
    auto table = readTable(path);
    if (!table) return strings;
    for (const auto &[key, node] : *table) {
        if (auto value = node.value<std::string>()) strings[std::string(key.str())] = *value;
    }
    return strings;
}

void loadTranslations(PluginPackage &package, const std::vector<std::string> &preferred)
{
    fs::path folder = fs::path(package.directory) / "i18n";
    std::vector<std::string> available, english;
    std::error_code error;
    for (const auto &entry : fs::directory_iterator(folder, error)) {
        if (entry.path().extension() != ".toml") continue;
        std::string locale = entry.path().stem().string();
        available.push_back(locale);
        if (locale.rfind("en", 0) == 0) english.push_back(locale);
    }
    if (auto locale = lua::bestLocale(available, preferred)) {
        package.locale = *locale;
        package.strings = loadStrings(folder / (*locale + ".toml"));
    }
    if (auto fallback = lua::bestLocale(english, preferred)) package.fallbackStrings = loadStrings(folder / (*fallback + ".toml"));
}

bool loadModules(PluginPackage &package, size_t limit)
{
    fs::path root(package.directory);
    size_t total = 0;
    std::error_code error;
    for (auto walker = fs::recursive_directory_iterator(root, error); walker != fs::recursive_directory_iterator();
         walker.increment(error)) {
        if (error) return false;
        fs::path relative = fs::relative(walker->path(), root);
        if (relative.begin() != relative.end() && *relative.begin() == "tests") {
            walker.disable_recursion_pending();
            continue;
        }
        if (walker.depth() > 4 || !walker->is_regular_file() || relative.extension() != ".lua" || relative == package.main) continue;
        auto contents = readFile(walker->path(), limit);
        if (!contents) return false;
        total += contents->size();
        if (total > limit) return false;
        std::string name = relative.replace_extension().generic_string();
        for (char &character : name) {
            if (character == '/') character = '.';
        }
        package.modules[name] = *contents;
        fs::path compiled = walker->path();
        compiled.replace_extension(".luac");
        if (auto bytecode = readFile(compiled, limit)) package.moduleBytecode[name] = *bytecode;
    }
    return true;
}
} // namespace

std::optional<PluginPackage> loadPluginPackage(const std::string &directory, const std::vector<std::string> &preferredLanguages,
                                               PackageLimits limits)
{
    auto manifest = readTable(fs::path(directory) / "manifest.toml");
    if (!manifest) return std::nullopt;
    PluginPackage package;
    package.identifier = stringAt(*manifest, "id");
    package.name = stringAt(*manifest, "name");
    package.main = stringAt(*manifest, "main", "main.lua");
    if (!lua::validPluginIdentifier(package.identifier) || package.name.empty() || package.main.find('/') != std::string::npos)
        return std::nullopt;
    auto source = readFile(fs::path(directory) / package.main, limits.source);
    if (!source) return std::nullopt;
    package.source = *source;
    fs::path compiledMain = fs::path(directory) / package.main;
    compiledMain.replace_extension(".luac");
    if (auto bytecode = readFile(compiledMain, limits.source * 2)) package.mainBytecode = *bytecode;
    package.directory = directory;
    package.name = truncateCodepoints(package.name, limits.nameLength);
    package.version = stringAt(*manifest, "version");
    package.description = stringAt(*manifest, "description");
    package.icon = stringAt(*manifest, "icon");
    package.enabledByDefault = (*manifest)["enabled_by_default"].value_or(false);
    std::string iconSvg = stringAt(*manifest, "iconSvg");
    if (iconSvg.size() < limits.iconSvg) package.iconSvg = iconSvg;
    if (auto authors = (*manifest)["authors"].as_array()) {
        for (const auto &author : *authors) {
            if (package.authors.size() >= limits.authors) break;
            if (auto name = author.value<std::string>()) {
                package.authors.push_back({*name, ""});
            } else if (auto table = author.as_table()) {
                if (auto tableName = (*table)["name"].value<std::string>()) package.authors.push_back({*tableName, stringAt(*table, "id")});
            }
        }
    }
    if (auto permissions = (*manifest)["permissions"].as_array()) {
        for (const auto &permission : *permissions) {
            if (auto value = permission.value<std::string>()) package.permissions.insert(*value);
        }
    }
    loadTranslations(package, preferredLanguages);
    if (!loadModules(package, limits.modules)) return std::nullopt;
    return package;
}

std::string PluginPackage::translate(const std::string &key, const std::vector<std::pair<std::string, std::string>> &values) const
{
    auto found = strings.find(key);
    std::string text = key;
    if (found != strings.end()) {
        text = found->second;
    } else if (auto fallback = fallbackStrings.find(key); fallback != fallbackStrings.end()) {
        text = fallback->second;
    }
    return lua::fillPlaceholders(text, values);
}

void PluginHealth::reset()
{
    errors = 0;
    failed = false;
    lastError.clear();
}

bool PluginHealth::recordError(const std::string &stage, const std::string &message, int maximumErrors)
{
    lastError = stage + ": " + message;
    return ++errors >= maximumErrors && markFailed();
}

bool PluginHealth::markFailed()
{
    if (failed) return false;
    failed = true;
    return true;
}
} // namespace astral
