#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>
#include <nlohmann/json.hpp>

#include "AstralRust.h"
#include "AstralSettings.h"
#include "PhoenixWire.h"

extern "C" {
AstralRustString astral_themes_list(const char *state, size_t stateLength, const char *resources, size_t resourcesLength);
AstralRustString astral_themes_select(const AstralSettingsHandle *settings, const char *state, size_t stateLength, const char *resources,
                                      size_t resourcesLength, const char *identifier, size_t identifierLength);
AstralRustString astral_themes_import_unbound(const AstralSettingsHandle *settings, const char *state, size_t stateLength,
                                              const char *resources, size_t resourcesLength, const char *source, size_t sourceLength,
                                              const char *fallback, size_t fallbackLength);
}

namespace astral {

struct ThemeStyle {
    std::vector<uint32_t> from;
    std::vector<uint32_t> to;
    std::vector<uint32_t> gradient;
    double radius = 0;
    double font = 0;
    uint32_t tint = 0;
    double saturation = 0;
    double neutral = 0;
    std::array<uint32_t, 3> surfaces{};
    std::array<double, 3> surfaceSaturation{};

    void apply() const
    {
        astral_wire_set_palette(from.data(), to.data(), from.size());
        astral_wire_set_scales(radius, font);
        astral_wire_set_background_gradient(gradient.data(), gradient.size());
        astral_wire_set_tint(tint, saturation, neutral);
        for (size_t index = 0; index < surfaces.size(); index++)
            if (surfaces[index]) astral_wire_set_surface_tint(static_cast<AstralSurface>(index), surfaces[index], surfaceSaturation[index]);
    }
};

class Themes
{
  public:
    Themes(SettingsStore &settings, std::string stateDirectory, std::string resourceDirectory)
        : settings_(settings), state_(std::move(stateDirectory)), resources_(std::move(resourceDirectory))
    {
    }

    nlohmann::json list() const
    {
        return nlohmann::json::parse(rust::take(astral_themes_list(state_.data(), state_.size(), resources_.data(), resources_.size())),
                                     nullptr, false);
    }

    std::string active() const { return settings_.getString("astral", "theme", "discord"); }

    ThemeStyle select(const std::string &identifier)
    {
        nlohmann::json style =
            nlohmann::json::parse(rust::take(astral_themes_select(settings_.handle(), state_.data(), state_.size(), resources_.data(),
                                                                  resources_.size(), identifier.data(), identifier.size())),
                                  nullptr, false);
        ThemeStyle result;
        if (!style.is_object()) return result;
        result.from = style.value("from", std::vector<uint32_t>{});
        result.to = style.value("to", std::vector<uint32_t>{});
        result.gradient = style.value("gradient", std::vector<uint32_t>{});
        result.radius = style.value("radius", 0.0);
        result.font = style.value("font", 0.0);
        result.tint = style.value("tint", 0u);
        result.saturation = style.value("saturation", 0.0);
        result.neutral = style.value("neutral", 0.0);
        const nlohmann::json &surfaces = style["surfaces"];
        for (size_t index = 0; surfaces.is_array() && index < result.surfaces.size() && index < surfaces.size(); index++) {
            result.surfaces[index] = surfaces[index].value("color", 0u);
            result.surfaceSaturation[index] = surfaces[index].value("saturation", 0.0);
        }
        return result;
    }

    std::optional<std::string> importUnbound(const std::string &source, const std::string &fallbackName)
    {
        return rust::takeIfPresent(astral_themes_import_unbound(settings_.handle(), state_.data(), state_.size(), resources_.data(),
                                                                resources_.size(), source.data(), source.size(), fallbackName.data(),
                                                                fallbackName.size()));
    }

    std::string directory() const { return state_ + (state_.empty() || state_.back() == '/' ? "" : "/") + "themes"; }

  private:
    SettingsStore &settings_;
    std::string state_;
    std::string resources_;
};
}
