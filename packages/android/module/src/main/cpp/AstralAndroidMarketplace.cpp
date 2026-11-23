#include "AstralAndroidMarketplace.h"

#include <filesystem>
#include <fstream>
#include <exception>
#include <thread>
#include <utility>

#include "AstralDiscordCache.h"
#include "AstralFiles.h"
#include "AstralLog.h"
#include "AstralLuaHost.h"
#include "AstralSettings.h"
#include "AstralSignatures.h"
#include "AstralThemes.h"
#include "AstralTOML.h"
#include "LuaLocale.h"

namespace {

nlohmann::json catalogEntries(const nlohmann::json &source, const std::string &key)
{
    nlohmann::json result = nlohmann::json::array();
    auto found = source.find(key);
    if (found == source.end() || !found->is_array()) return result;
    for (const auto &entry : *found) {
        if (!entry.is_object() || !entry.contains("id") || !entry["id"].is_string() || !entry.contains("name") ||
            !entry["name"].is_string() || !entry.contains("base") || !entry["base"].is_string())
            continue;
        result.push_back(entry);
    }
    return result;
}

std::optional<std::string> readLimitedFile(const std::string &path, size_t maximum)
{
    std::error_code error;
    uint64_t size = std::filesystem::file_size(path, error);
    if (error || size > maximum) return std::nullopt;
    std::ifstream input(path, std::ios::binary);
    if (!input) return std::nullopt;
    std::string contents(static_cast<size_t>(size), '\0');
    if (size && !input.read(contents.data(), static_cast<std::streamsize>(size))) return std::nullopt;
    if (input.peek() != std::char_traits<char>::eof()) return std::nullopt;
    return contents;
}

bool safeRelativePath(const std::string &relative)
{
    if (relative.empty() || relative.size() > 512 || relative.find('\\') != std::string::npos || relative.find('\0') != std::string::npos)
        return false;
    std::filesystem::path path(relative);
    if (path.is_absolute() || path.lexically_normal().generic_string() != relative) return false;
    for (const auto &part : path)
        if (part == "." || part == "..") return false;
    return true;
}

std::string defaultAvatar(size_t index)
{
    return "https://cdn.discordapp.com/embed/avatars/" + std::to_string(index % 6) + ".png";
}

}

astral::android::Marketplace::Marketplace(SettingsStore &settings, Themes &themes, Signatures &signatures, std::string stateDirectory,
                                          std::string resourceDirectory, Fetch fetch, ApplyTheme applyTheme)
    : settings_(settings), themes_(themes), signatures_(signatures), stateDirectory_(std::move(stateDirectory)),
      resourceDirectory_(std::move(resourceDirectory)), fetch_(std::move(fetch)), applyTheme_(std::move(applyTheme))
{
}

nlohmann::json astral::android::Marketplace::catalog() const
{
    std::lock_guard<std::mutex> lock(mutex_);
    return catalog_;
}

std::string astral::android::Marketplace::catalogState() const
{
    std::lock_guard<std::mutex> lock(mutex_);
    return state_;
}

std::string astral::android::Marketplace::catalogError() const
{
    std::lock_guard<std::mutex> lock(mutex_);
    return error_;
}

std::string astral::android::Marketplace::catalogURL() const
{
    return settings_.getString("astral", "catalog", "file://" + resourceDirectory_ + "/catalog/index.toml");
}

std::optional<std::string> astral::android::Marketplace::installStatus(const std::string &identifier) const
{
    std::lock_guard<std::mutex> lock(mutex_);
    auto found = installStatuses_.find(identifier);
    return found == installStatuses_.end() ? std::nullopt : std::optional<std::string>(found->second);
}

std::optional<std::string> astral::android::Marketplace::fetchContent(const std::string &url, size_t maximum, std::string &failure) const
{
    if (url.rfind("file://", 0) == 0) {
        auto contents = readLimitedFile(url.substr(7), maximum);
        if (!contents) failure = "could not read " + url.substr(7);
        return contents;
    }
    if (url.rfind("https://", 0) != 0) {
        failure = "downloads must use https";
        return std::nullopt;
    }
    size_t authorityEnd = url.find_first_of("/?#", 8);
    std::string authority = authorityEnd == std::string::npos ? url.substr(8) : url.substr(8, authorityEnd - 8);
    if (authority.empty() || authority.find('@') != std::string::npos || url.find('\\') != std::string::npos) {
        failure = "invalid https URL";
        return std::nullopt;
    }
    auto response = fetch_ ? fetch_(url, "") : reactors::FetchResult{};
    if (response.status != 200) {
        failure = response.status ? "HTTP " + std::to_string(response.status) : "request failed";
        return std::nullopt;
    }
    if (response.body.size() > maximum) {
        failure = "download too large";
        return std::nullopt;
    }
    return response.body;
}

std::optional<std::string> astral::android::Marketplace::resolveCatalogReference(const std::string &reference) const
{
    if (reference.empty() || reference.size() > 4096 || reference.find('\\') != std::string::npos ||
        reference.find('\0') != std::string::npos)
        return std::nullopt;
    if (reference.find("://") != std::string::npos) return reference;
    if (!safeRelativePath(reference)) return std::nullopt;
    std::string source = catalogURL();
    if (source.rfind("file://", 0) == 0) {
        std::filesystem::path parent = std::filesystem::path(source.substr(7)).parent_path().lexically_normal();
        std::filesystem::path resolved = (parent / reference).lexically_normal();
        std::filesystem::path relative = resolved.lexically_relative(parent);
        if (relative.empty() || relative.is_absolute()) return std::nullopt;
        for (const auto &part : relative)
            if (part == "..") return std::nullopt;
        return "file://" + resolved.string();
    }
    if (source.rfind("https://", 0) != 0) return std::nullopt;
    size_t authorityEnd = source.find_first_of("/?#", 8);
    std::string authority = authorityEnd == std::string::npos ? source.substr(8) : source.substr(8, authorityEnd - 8);
    if (authority.empty() || authority.find('@') != std::string::npos) return std::nullopt;
    size_t slash = source.find_last_of('/');
    std::string directory = slash == std::string::npos || slash < 8 ? source + "/" : source.substr(0, slash + 1);
    return directory + reference;
}

std::optional<std::string> astral::android::Marketplace::fetchPackagePath(const std::string &base, const std::string &path, size_t maximum,
                                                                          std::string &failure) const
{
    auto resolved = resolveCatalogReference(base);
    if (!resolved) {
        failure = "invalid package URL";
        return std::nullopt;
    }
    std::string url = *resolved;
    if (!path.empty()) {
        if (url.back() != '/') url.push_back('/');
        url += path;
    }
    return fetchContent(url, maximum, failure);
}

std::optional<nlohmann::json> astral::android::Marketplace::parseVerifiedCatalog(const std::string &contents,
                                                                                 const std::string &signature) const
{
    if (!signatures_.verify(contents, signature)) return std::nullopt;
    nlohmann::json parsed = parseTOML(contents);
    if (!parsed.is_object()) return std::nullopt;
    nlohmann::json result = nlohmann::json::object();
    result["astral"] = parsed.value("astral", nlohmann::json::object());
    if (!result["astral"].is_object()) result["astral"] = nlohmann::json::object();
    result["compatibility"] = parsed.value("compatibility", nlohmann::json::object());
    if (!result["compatibility"].is_object()) result["compatibility"] = nlohmann::json::object();
    result["plugins"] = catalogEntries(parsed, "plugins");
    result["themes"] = catalogEntries(parsed, "themes");
    return result;
}

std::optional<nlohmann::json> astral::android::Marketplace::downloadCatalog(const std::string &url, std::string &failure) const
{
    auto contents = fetchContent(url, 512u << 10, failure);
    if (!contents) return std::nullopt;
    auto signature = fetchContent(url + ".sig", 512, failure);
    if (!signature) return std::nullopt;
    auto result = parseVerifiedCatalog(*contents, *signature);
    if (!result) {
        failure = "catalog signature or TOML is invalid";
        return std::nullopt;
    }
    if (url.rfind("https://", 0) == 0) {
        std::string cache = stateDirectory_ + "/catalog";
        createDirectories(cache);
        writeTextFileAtomically(cache + "/index.toml", *contents);
        writeTextFileAtomically(cache + "/index.toml.sig", *signature);
    }
    return result;
}

std::optional<nlohmann::json> astral::android::Marketplace::cachedCatalog() const
{
    auto contents = readTextFile(stateDirectory_ + "/catalog/index.toml");
    auto signature = readTextFile(stateDirectory_ + "/catalog/index.toml.sig");
    return contents && signature ? parseVerifiedCatalog(*contents, *signature) : std::nullopt;
}

void astral::android::Marketplace::refreshCatalog(const std::function<void()> &changed)
{
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (loading_) return;
        loading_ = true;
        state_ = "loading";
        error_.clear();
    }
    if (changed) changed();
    std::string url = catalogURL();
    std::thread([this, url = std::move(url), changed] {
        std::string failure;
        auto downloaded = downloadCatalog(url, failure);
        auto result = downloaded ? downloaded : cachedCatalog();
        {
            std::lock_guard<std::mutex> lock(mutex_);
            if (result) catalog_ = std::move(*result);
            state_ = result || !catalog_.empty() ? "ready" : "failed";
            error_ = std::move(failure);
            loading_ = false;
        }
        if (changed) changed();
    }).detach();
}

bool astral::android::Marketplace::validPackageIdentifier(const std::string &identifier) const
{
    if (identifier.empty() || identifier.size() > 128 || identifier.find("..") != std::string::npos) return false;
    for (unsigned char character : identifier) {
        if (!((character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') || (character >= '0' && character <= '9') ||
              character == '.' || character == '_' || character == '-'))
            return false;
    }
    return identifier != "." && identifier != "..";
}

bool astral::android::Marketplace::installTheme(const nlohmann::json &package, std::string &failure)
{
    std::string identifier = package.value("id", "");
    std::string base = package.value("base", "");
    if (!validPackageIdentifier(identifier) || base.empty()) {
        failure = "invalid theme package";
        return false;
    }
    auto contents = fetchPackagePath(base, "", 1u << 20, failure);
    if (!contents) return false;
    nlohmann::json theme = parseTOML(*contents);
    if (!theme.is_object() || theme.value("id", std::string()) != identifier || !theme.contains("colors") || !theme["colors"].is_object()) {
        failure = "theme does not match the catalog";
        return false;
    }
    std::string directory = themes_.directory();
    if (!createDirectories(directory) || !writeTextFileAtomically(directory + "/" + identifier + ".toml", *contents)) {
        failure = "could not write theme";
        return false;
    }
    return true;
}

bool astral::android::Marketplace::installPlugin(const nlohmann::json &package, std::string &failure)
{
    std::string identifier = package.value("id", "");
    std::string base = package.value("base", "");
    if (!lua::validPluginIdentifier(identifier) || base.empty()) {
        failure = "invalid plugin package";
        return false;
    }
    auto signature = fetchPackagePath(base, "signature", 128u << 10, failure);
    if (!signature) return false;
    auto files = signatures_.files(*signature);
    if (!files) {
        failure = "plugin signature is invalid";
        return false;
    }
    std::string root = stateDirectory_ + "/plugins";
    std::string staging = root + "/." + identifier + ".installing";
    std::string target = root + "/" + identifier;
    std::string previous = root + "/." + identifier + ".previous";
    std::error_code error;
    std::filesystem::remove_all(staging, error);
    if (error || !createDirectories(staging) || !writeTextFileAtomically(staging + "/signature", *signature)) {
        failure = "could not prepare plugin files";
        std::filesystem::remove_all(staging, error);
        return false;
    }
    size_t total = 0;
    for (const auto &file : *files) {
        const std::string &path = file.first;
        auto contents = fetchPackagePath(base, path, 1u << 20, failure);
        if (!contents || (total += contents->size()) > (2u << 20)) {
            if (contents) failure = "plugin download exceeds the size limit";
            std::filesystem::remove_all(staging, error);
            return false;
        }
        auto destination = resolveInsideDirectory(staging, path);
        if (!destination || !createDirectories(std::filesystem::path(*destination).parent_path().string()) ||
            !writeTextFileAtomically(*destination, *contents)) {
            failure = "could not write plugin files";
            std::filesystem::remove_all(staging, error);
            return false;
        }
    }
    auto manifestText = readTextFile(staging + "/manifest.toml");
    auto manifest = manifestText ? parseTOML(*manifestText) : nlohmann::json();
    std::string main = manifest.is_object() ? manifest.value("main", "main.lua") : "";
    if (!manifest.is_object() || manifest.value("id", std::string()) != identifier || main.find('/') != std::string::npos ||
        !files->count(main) || !signatures_.directory(staging, main)) {
        failure = "downloaded plugin files do not match their signature";
        std::filesystem::remove_all(staging, error);
        return false;
    }
    std::filesystem::remove_all(previous, error);
    error.clear();
    if (std::filesystem::exists(target, error)) {
        error.clear();
        std::filesystem::rename(target, previous, error);
        if (error) {
            failure = "could not replace the installed plugin";
            std::filesystem::remove_all(staging, error);
            return false;
        }
    }
    error.clear();
    std::filesystem::rename(staging, target, error);
    if (error) {
        std::error_code restore;
        if (std::filesystem::exists(previous, restore)) std::filesystem::rename(previous, target, restore);
        failure = "could not install the plugin";
        std::filesystem::remove_all(staging, restore);
        return false;
    }
    std::filesystem::remove_all(previous, error);
    luahost::remove(identifier);
    luahost::rescan();
    return true;
}

void astral::android::Marketplace::installPackage(const nlohmann::json &package, const std::string &kind,
                                                  const std::function<void()> &changed)
{
    if (!package.is_object()) return;
    std::string identifier = package.value("id", "");
    if (!validPackageIdentifier(identifier) || (kind != "plugin" && kind != "theme")) return;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto found = installStatuses_.find(identifier);
        if (found != installStatuses_.end() && found->second == "Installing…") return;
        installStatuses_[identifier] = "Installing…";
    }
    if (changed) changed();
    std::thread([this, package, kind, identifier, changed] {
        std::string failure;
        bool installed = false;
        try {
            installed = kind == "theme" ? installTheme(package, failure) : installPlugin(package, failure);
        } catch (const std::exception &error) {
            failure = error.what();
        }
        {
            std::lock_guard<std::mutex> lock(mutex_);
            if (installed)
                installStatuses_.erase(identifier);
            else
                installStatuses_[identifier] = "Failed";
        }
        if (!installed) log(LogLevel::Error, "marketplace", "Install failed for " + identifier + ": " + failure);
        if (installed && kind == "theme" && themes_.active() == identifier && applyTheme_) applyTheme_(identifier);
        if (changed) changed();
    }).detach();
}

std::string astral::android::Marketplace::authorAvatar(const std::string &userID, size_t index) const
{
    if (userID.size() < 15 || userID.size() > 20 || userID.find_first_not_of("0123456789") != std::string::npos)
        return defaultAvatar(index);
    auto user = discord::user(userID);
    if (!user || !user->is_object() || !user->contains("avatar") || !(*user)["avatar"].is_string() ||
        (*user)["avatar"].get<std::string>().empty())
        return defaultAvatar(index);
    return "https://cdn.discordapp.com/avatars/" + userID + "/" + (*user)["avatar"].get<std::string>() + ".png?size=64";
}

std::string astral::android::Marketplace::pluginIconPath(const std::string &name)
{
    constexpr std::string_view prefix = "astral_plugin_";
    if (name.rfind(prefix, 0) != 0) return "";
    std::string identifier = name.substr(prefix.size());
    if (!lua::validPluginIdentifier(identifier)) return "";
    for (const auto &plugin : luahost::plugins()) {
        if (plugin.value("id", "") != identifier) continue;
        std::string svg = plugin.value("iconSvg", "");
        if (svg.empty() || svg.size() > 16384) return "";
        std::string directory = stateDirectory_ + "/plugin-icons";
        std::string path = directory + "/" + identifier + ".svg";
        auto cached = readTextFile(path);
        if (!cached || *cached != svg) {
            if (!createDirectories(directory) || !writeTextFileAtomically(path, svg)) return "";
        }
        return path;
    }
    return "";
}
