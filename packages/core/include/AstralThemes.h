#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>
#include <nlohmann/json.hpp>

#include "PhoenixWire.h"

namespace astral {
class SettingsStore;

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
    Themes(SettingsStore &settings, std::string stateDirectory, std::string resourceDirectory);
    nlohmann::json list() const;
    std::string active() const;
    ThemeStyle select(const std::string &identifier);
    std::optional<std::string> importUnbound(const std::string &source, const std::string &fallbackName);
    std::string directory() const;

  private:
    nlohmann::json palette(const std::string &name) const;
    ThemeStyle style(const nlohmann::json &theme) const;
    SettingsStore &settings_;
    std::string state_;
    std::string resources_;
};
}
