#pragma once

#include <nlohmann/json.hpp>
#include <string>

#include "AstralRust.h"
#include "AstralSettings.h"

extern "C" {
typedef struct AstralPacksHandle AstralPacksHandle;
AstralPacksHandle *astral_packs_open(const AstralSettingsHandle *settings, const char *state, size_t stateLength, const char *resources,
                                     size_t resourcesLength);
void astral_packs_close(AstralPacksHandle *packs);
AstralRustString astral_packs_list(const AstralPacksHandle *packs, int emoji);
int astral_packs_select(const AstralPacksHandle *packs, int emoji, const char *identifier, size_t length);
void astral_packs_invalidate(const AstralPacksHandle *packs);
AstralRustString astral_packs_icon_path(const AstralPacksHandle *packs, const char *name, size_t length);
AstralRustString astral_packs_emoji_url(const AstralPacksHandle *packs, const char *codepoints, size_t length);
}

namespace astral {
class Packs
{
  public:
    Packs(SettingsStore &settings, const std::string &stateDirectory, const std::string &resourceDirectory)
        : settings_(settings), handle_(astral_packs_open(settings.handle(), stateDirectory.data(), stateDirectory.size(),
                                                         resourceDirectory.data(), resourceDirectory.size()))
    {
    }

    ~Packs() { astral_packs_close(handle_); }

    Packs(const Packs &) = delete;
    Packs &operator=(const Packs &) = delete;

    nlohmann::json icons() const { return list(0); }
    nlohmann::json emoji() const { return list(1); }
    std::string activeIcons() const { return settings_.getString("astral", "icon-pack", "discord"); }
    std::string activeEmoji() const { return settings_.getString("astral", "emoji-pack", "discord"); }
    bool selectIcons(const std::string &identifier) { return astral_packs_select(handle_, 0, identifier.data(), identifier.size()); }
    bool selectEmoji(const std::string &identifier) { return astral_packs_select(handle_, 1, identifier.data(), identifier.size()); }
    bool animated() const { return settings_.getBoolean("astral", "emoji-animated", true); }

    void setAnimated(bool enabled)
    {
        if (settings_.set("astral", "emoji-animated", enabled)) settings_.save();
    }

    std::string iconPath(const std::string &name) { return rust::take(astral_packs_icon_path(handle_, name.data(), name.size())); }

    std::string emojiURL(const std::string &codepoints)
    {
        return rust::take(astral_packs_emoji_url(handle_, codepoints.data(), codepoints.size()));
    }

    void invalidate() { astral_packs_invalidate(handle_); }

  private:
    nlohmann::json list(int emoji) const
    {
        nlohmann::json parsed = nlohmann::json::parse(rust::take(astral_packs_list(handle_, emoji)), nullptr, false);
        return parsed.is_array() ? parsed : nlohmann::json::array();
    }

    SettingsStore &settings_;
    AstralPacksHandle *handle_;
};
} // namespace astral
