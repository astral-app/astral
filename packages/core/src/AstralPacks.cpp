#include "AstralPacks.h"

#include <algorithm>
#include <cctype>
#include <sstream>

#include "AstralFiles.h"
#include "AstralSettings.h"
#include "AstralTOML.h"

namespace {
using nlohmann::json;

bool safeName(const std::string &name)
{
    return !name.empty() && name != "." && name.find("..") == std::string::npos && name.find_first_of("/\\\0", 0, 3) == std::string::npos;
}

std::string text(const json &object, const std::string &key)
{
    auto entry = object.find(key);
    return entry != object.end() && entry->is_string() ? entry->get<std::string>() : "";
}

std::set<std::string> coverage(const std::string &directory, const std::string &name)
{
    auto path = astral::resolveInsideDirectory(directory, name);
    auto contents = path ? astral::readTextFile(*path) : std::nullopt;
    std::istringstream lines(contents.value_or(""));
    std::set<std::string> result;
    std::string line;
    while (std::getline(lines, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (!line.empty()) result.insert(line);
    }
    return result;
}

std::string coverageKey(const std::string &codepoints)
{
    std::istringstream parts(codepoints);
    std::string point, key;
    while (std::getline(parts, point, '-')) {
        if (point == "200d")
            key += '+';
        else if (point == "fe0f")
            key += '!';
        else {
            if (!key.empty() && key.back() != '+') key += '-';
            key += point;
        }
    }
    return key;
}
}

astral::Packs::Packs(SettingsStore &settings, std::string stateDirectory, std::string resourceDirectory)
    : settings_(settings), state_(std::move(stateDirectory)), resources_(std::move(resourceDirectory))
{
}

nlohmann::json astral::Packs::list(const std::string &kind) const
{
    bool emoji = kind == "emoji-packs";
    json result = json::array({{{"id", "discord"},
                                {"name", "Discord"},
                                {"description", emoji ? "Discord's own emoji" : "Discord's own icons"},
                                {"bundled", true}}});
    std::set<std::string> seen{"discord"};
    for (const auto &root : {state_, resources_}) {
        auto directory = joinPath(root, kind);
        auto entries = listDirectory(directory);
        std::sort(entries.begin(), entries.end());
        for (const auto &entry : entries) {
            auto pack = resolveInsideDirectory(directory, entry);
            auto path = pack ? resolveInsideDirectory(*pack, "manifest.toml") : std::nullopt;
            auto contents = path ? readTextFile(*path) : std::nullopt;
            auto manifest = contents ? parseTOML(*contents) : json();
            if (!manifest.is_object()) continue;
            auto id = text(manifest, "id");
            if (!safeName(id) || text(manifest, "name").empty() || seen.count(id)) continue;
            if (emoji && text(manifest, "static").rfind("https://", 0) != 0) continue;
            manifest["directory"] = *pack;
            manifest["bundled"] = root == resources_;
            if (!emoji) manifest["count"] = listDirectory(joinPath(*pack, "icons")).size();
            seen.insert(id);
            result.push_back(std::move(manifest));
        }
    }
    return result;
}

nlohmann::json astral::Packs::icons() const
{
    return list("icon-packs");
}
nlohmann::json astral::Packs::emoji() const
{
    return list("emoji-packs");
}
std::string astral::Packs::activeIcons() const
{
    return settings_.getString("astral", "icon-pack", "discord");
}
std::string astral::Packs::activeEmoji() const
{
    return settings_.getString("astral", "emoji-pack", "discord");
}
bool astral::Packs::animated() const
{
    return settings_.getBoolean("astral", "emoji-animated", true);
}

bool astral::Packs::select(const std::string &kind, const std::string &key, const std::string &identifier)
{
    for (const auto &pack : list(kind)) {
        if (text(pack, "id") != identifier) continue;
        settings_.set("astral", key, identifier);
        if (!settings_.save()) return false;
        invalidate();
        return true;
    }
    return false;
}

bool astral::Packs::selectIcons(const std::string &identifier)
{
    return select("icon-packs", "icon-pack", identifier);
}
bool astral::Packs::selectEmoji(const std::string &identifier)
{
    return select("emoji-packs", "emoji-pack", identifier);
}

void astral::Packs::setAnimated(bool enabled)
{
    if (settings_.set("astral", "emoji-animated", enabled)) settings_.save();
}

void astral::Packs::invalidate()
{
    std::lock_guard<std::mutex> lock(mutex_);
    loadedEmoji_.clear();
    manifest_ = nullptr;
    coverage_.clear();
    animatedCoverage_.clear();
    loadedIcons_.clear();
    iconDirectory_.clear();
    iconPaths_.clear();
}

std::string astral::Packs::iconPath(const std::string &name)
{
    if (name.rfind("ic_", 0) != 0 || !safeName(name)) return "";
    auto active = activeIcons();
    if (active == "discord") return "";
    std::lock_guard<std::mutex> lock(mutex_);
    if (loadedIcons_ != active) {
        loadedIcons_ = active;
        iconDirectory_.clear();
        iconPaths_.clear();
        for (const auto &pack : icons()) {
            if (text(pack, "id") == active) iconDirectory_ = text(pack, "directory");
        }
    }
    auto cached = iconPaths_.find(name);
    if (cached != iconPaths_.end()) return cached->second;
    if (iconPaths_.size() >= 4096) iconPaths_.clear();
    for (int scale : {3, 2, 1}) {
        auto path = iconDirectory_.empty()
                        ? std::nullopt
                        : resolveInsideDirectory(iconDirectory_, "icons/" + name + "@" + std::to_string(scale) + "x.png");
        if (path && fileExists(*path)) return iconPaths_[name] = *path;
    }
    iconPaths_[name] = "";
    return "";
}

std::string astral::Packs::emojiURL(const std::string &codepoints)
{
    if (codepoints.empty() || codepoints.size() > 96) return "";
    std::string lower = codepoints;
    for (auto &c : lower) {
        c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
        if (!std::isxdigit(static_cast<unsigned char>(c)) && c != '-') return "";
    }
    auto active = activeEmoji();
    if (active == "discord") return "";
    bool animate = animated();
    std::lock_guard<std::mutex> lock(mutex_);
    if (loadedEmoji_ != active) {
        loadedEmoji_ = active;
        manifest_ = nullptr;
        coverage_.clear();
        animatedCoverage_.clear();
        for (const auto &pack : emoji()) {
            if (text(pack, "id") != active) continue;
            manifest_ = pack;
            coverage_ = coverage(text(pack, "directory"), "coverage.txt");
            animatedCoverage_ = coverage(text(pack, "directory"), "animated.txt");
            break;
        }
    }
    auto key = coverageKey(lower);
    if (!manifest_.is_object() || !coverage_.count(key)) return "";
    auto pattern = text(manifest_, "static");
    auto animatedPattern = text(manifest_, "animated");
    if (animate && animatedCoverage_.count(key) && animatedPattern.rfind("https://", 0) == 0) pattern = animatedPattern;
    size_t position = 0;
    while ((position = pattern.find("{codepoints}", position)) != std::string::npos) {
        pattern.replace(position, 12, lower);
        position += lower.size();
    }
    return pattern;
}
