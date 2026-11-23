#pragma once

#include <mutex>
#include <map>
#include <set>
#include <string>
#include <nlohmann/json.hpp>

namespace astral {
class SettingsStore;

class Packs
{
  public:
    Packs(SettingsStore &settings, std::string stateDirectory, std::string resourceDirectory);
    nlohmann::json icons() const;
    nlohmann::json emoji() const;
    std::string activeIcons() const;
    std::string activeEmoji() const;
    bool selectIcons(const std::string &identifier);
    bool selectEmoji(const std::string &identifier);
    bool animated() const;
    void setAnimated(bool enabled);
    std::string iconPath(const std::string &name);
    std::string emojiURL(const std::string &codepoints);
    void invalidate();

  private:
    nlohmann::json list(const std::string &kind) const;
    bool select(const std::string &kind, const std::string &key, const std::string &identifier);
    SettingsStore &settings_;
    std::string state_;
    std::string resources_;
    std::mutex mutex_;
    std::string loadedEmoji_;
    nlohmann::json manifest_;
    std::set<std::string> coverage_;
    std::set<std::string> animatedCoverage_;
    std::string loadedIcons_;
    std::string iconDirectory_;
    std::map<std::string, std::string> iconPaths_;
};
}
