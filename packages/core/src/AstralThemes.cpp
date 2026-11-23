#include "AstralThemes.h"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstdio>
#include <map>
#include <set>
#include <sstream>

#include "AstralFiles.h"
#include "AstralJSON.h"
#include "AstralSettings.h"
#include "AstralTOML.h"

namespace {
using nlohmann::json;

std::string text(const json &object, const std::string &key)
{
    auto entry = object.find(key);
    return entry != object.end() && entry->is_string() ? entry->get<std::string>() : "";
}

json object(const json &parent, const std::string &key)
{
    auto entry = parent.find(key);
    return entry != parent.end() && entry->is_object() ? *entry : json::object();
}

double number(const json &parent, const std::string &key)
{
    auto entry = parent.find(key);
    return entry != parent.end() && entry->is_number() ? entry->get<double>() : 0;
}

uint32_t color(std::string hex)
{
    if (!hex.empty() && hex[0] == '#') hex.erase(0, 1);
    if (hex.size() == 6) hex = "FF" + hex;
    if (hex.size() != 8) return 0;
    uint32_t result = 0;
    auto [end, error] = std::from_chars(hex.data(), hex.data() + hex.size(), result, 16);
    return error == std::errc() && end == hex.data() + hex.size() ? result : 0;
}

std::string argb(std::string hex)
{
    if (!hex.empty() && hex[0] == '#') hex.erase(0, 1);
    if (hex.size() == 6) return "FF" + hex;
    if (hex.size() == 8) return hex.substr(6) + hex.substr(0, 6);
    return "";
}

uint32_t semantic(const json &entry, const std::string &theme, const json &raw)
{
    auto found = entry.find(theme);
    if (found == entry.end() || !found->is_array() || found->size() != 2 || !(*found)[0].is_string() || !(*found)[1].is_number()) return 0;
    auto base = color(argb(text(raw, (*found)[0].get<std::string>())));
    double opacity = std::clamp((*found)[1].get<double>(), 0.0, 1.0);
    uint32_t alpha = static_cast<uint32_t>(std::lround((base >> 24) * opacity));
    return (base & 0xFFFFFF) | (alpha << 24);
}

std::string hex(uint32_t value)
{
    char result[9];
    snprintf(result, sizeof(result), "%08X", value);
    return result;
}

std::string slug(const std::string &name)
{
    std::string result;
    for (unsigned char c : name) {
        if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
        if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9'))
            result += static_cast<char>(c);
        else if (!result.empty() && result.back() != '-')
            result += '-';
    }
    if (!result.empty() && result.back() == '-') result.pop_back();
    return result.empty() ? "imported" : result;
}
}

astral::Themes::Themes(SettingsStore &settings, std::string stateDirectory, std::string resourceDirectory)
    : settings_(settings), state_(std::move(stateDirectory)), resources_(std::move(resourceDirectory))
{
}

std::string astral::Themes::directory() const
{
    return joinPath(state_, "themes");
}
std::string astral::Themes::active() const
{
    return settings_.getString("astral", "theme", "discord");
}

nlohmann::json astral::Themes::list() const
{
    json result = json::array({{{"id", "discord"},
                                {"name", "Discord"},
                                {"description", "Discord's own colours"},
                                {"colors", json::object()},
                                {"bundled", true}}});
    std::set<std::string> seen{"discord"};
    for (const auto &root : {state_, resources_}) {
        auto folder = joinPath(root, "themes");
        for (const auto &name : listDirectory(folder)) {
            if (std::filesystem::path(name).extension() != ".toml") continue;
            auto path = resolveInsideDirectory(folder, name);
            auto contents = path ? readTextFile(*path) : std::nullopt;
            auto theme = contents ? parseTOML(*contents) : json();
            if (!theme.is_object() || text(theme, "id").empty() || text(theme, "name").empty() || !theme.contains("colors") ||
                !theme["colors"].is_object())
                continue;
            if (!seen.insert(text(theme, "id")).second) continue;
            theme["path"] = *path;
            theme["bundled"] = root == resources_;
            result.push_back(std::move(theme));
        }
    }
    return result;
}

nlohmann::json astral::Themes::palette(const std::string &name) const
{
    auto source = readTextFile(joinPath(resources_, "palettes/discord-" + name + ".toml"));
    return source ? object(parseTOML(*source), "colors") : json::object();
}

astral::ThemeStyle astral::Themes::style(const json &theme) const
{
    ThemeStyle result;
    std::map<uint32_t, uint32_t> mapped;
    auto colors = object(theme, "colors");
    for (const auto &[key, value] : colors.items()) {
        if (!value.is_string()) continue;
        uint32_t from = color(key), to = color(value.get<std::string>());
        if (!from || !to) continue;
        mapped[from] = to;
    }
    auto raw = palette("raw");
    auto tokens = palette("semantic");
    auto original = mapped;
    for (const auto &[key, entry] : tokens.items()) {
        auto found = original.find(semantic(entry, "dark", raw));
        if (found == original.end()) continue;
        for (const auto &base : {"ash", "onyx", "light"}) {
            auto source = semantic(entry, base, raw);
            if (source) mapped.emplace(source, found->second);
        }
    }
    for (const auto &[from, to] : mapped) {
        result.from.push_back(from);
        result.to.push_back(to);
    }
    result.radius = number(object(theme, "radius"), "scale");
    result.font = number(object(theme, "font"), "scale");
    auto background = object(theme, "background");
    if (background.contains("gradient") && background["gradient"].is_array()) {
        for (const auto &stop : background["gradient"]) {
            auto parsed = stop.is_string() ? color(stop.get<std::string>()) : 0;
            if (parsed) result.gradient.push_back(parsed);
        }
    }
    auto tint = object(theme, "tint");
    result.tint = color(text(tint, "color"));
    result.saturation = number(tint, "saturation");
    result.neutral = number(tint, "neutral");
    std::array<std::string, 3> names{"backgrounds", "text", "accents"};
    for (size_t index = 0; index < names.size(); index++) {
        auto surface = object(tint, names[index]);
        result.surfaces[index] = color(text(surface, "color"));
        result.surfaceSaturation[index] = number(surface, "saturation");
    }
    return result;
}

astral::ThemeStyle astral::Themes::select(const std::string &identifier)
{
    auto themes = list();
    json selected = themes.front();
    for (const auto &theme : themes)
        if (text(theme, "id") == identifier) selected = theme;
    auto result = style(selected);
    if (settings_.set("astral", "theme", text(selected, "id"))) settings_.save();
    return result;
}

std::optional<std::string> astral::Themes::importUnbound(const std::string &source, const std::string &fallbackName)
{
    auto parsed = parseBoundedJSON(source);
    if (!parsed.is_object()) return "not an Unbound theme";
    auto raw = object(parsed, "raw");
    if (raw.empty()) raw = object(parsed, "rawColors");
    auto tokens = object(parsed, "semantic");
    if (tokens.empty()) tokens = object(parsed, "semanticColors");
    if (raw.empty() && tokens.empty()) return "the theme has no raw or semantic colours to convert";
    auto base = palette("raw");
    auto semantics = palette("semantic");
    std::map<uint32_t, uint32_t> colors;
    for (const auto &[name, replacement] : raw.items()) {
        auto from = color(argb(text(base, name)));
        auto to = replacement.is_string() ? color(argb(replacement.get<std::string>())) : 0;
        if (from && to && from != to) colors[from] = to;
    }
    for (const auto &[name, replacement] : tokens.items()) {
        auto from = semantic(object(semantics, name), "dark", base);
        auto value = replacement.is_array() ? (replacement.empty() ? json() : replacement[0]) : replacement;
        auto to = value.is_string() ? color(argb(value.get<std::string>())) : 0;
        if (from && to && from != to) colors[from] = to;
    }
    if (colors.empty()) return "none of the theme's colours match Discord's palette";
    auto name = text(parsed, "name");
    if (name.empty()) name = fallbackName;
    std::string identifier = "unbound-" + slug(name);
    std::ostringstream out;
    out << "id = " << json(identifier).dump() << "\nname = " << json(name).dump()
        << "\nversion = '0.0.1'\ndescription = 'Imported from an Unbound theme'\n\n[colors]\n";
    std::array<uint32_t, 3> tints{};
    std::array<double, 3> chroma{};
    for (const auto &[from, to] : colors) {
        out << hex(from) << " = '" << hex(to) << "'\n";
        std::array<unsigned, 3> rgb{(to >> 16) & 255, (to >> 8) & 255, to & 255};
        double score = (*std::max_element(rgb.begin(), rgb.end()) - *std::min_element(rgb.begin(), rgb.end())) / 255.0;
        if (from == 0xFF5865F2) score += 1;
        auto surface = static_cast<size_t>(astral_wire_surface_of(from));
        if (score > chroma[surface]) {
            tints[surface] = to;
            chroma[surface] = score;
        }
    }
    uint32_t tint = tints[kAstralSurfaceAccents]       ? tints[kAstralSurfaceAccents]
                    : tints[kAstralSurfaceBackgrounds] ? tints[kAstralSurfaceBackgrounds]
                                                       : tints[kAstralSurfaceText];
    if (tint) out << "\n[tint]\ncolor = '#" << hex(tint).substr(2) << "'\nsaturation = 0.4\nneutral = 0.3\n";
    if (tints[kAstralSurfaceBackgrounds] && chroma[kAstralSurfaceBackgrounds] > 0.03)
        out << "\n[tint.backgrounds]\ncolor = '#" << hex(tints[kAstralSurfaceBackgrounds]).substr(2) << "'\nsaturation = 0.35\n";
    if (tints[kAstralSurfaceText] && chroma[kAstralSurfaceText] > 0.03)
        out << "\n[tint.text]\ncolor = '#" << hex(tints[kAstralSurfaceText]).substr(2) << "'\nsaturation = 0.4\n";
    if (!createDirectories(directory()) || !writeTextFileAtomically(joinPath(directory(), identifier + ".toml"), out.str()))
        return "could not save the theme";
    settings_.set("astral", "theme", identifier);
    if (!settings_.save()) return "could not select the imported theme";
    return std::nullopt;
}
