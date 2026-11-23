#pragma once

#include <functional>
#include <map>
#include <mutex>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

#include "AstralReactors.h"

namespace astral {
class SettingsStore;
class Signatures;
class Themes;
}

namespace astral::android {

class Marketplace
{
  public:
    using Fetch = std::function<reactors::FetchResult(const std::string &, const std::string &)>;
    using ApplyTheme = std::function<void(const std::string &)>;

    Marketplace(SettingsStore &settings, Themes &themes, Signatures &signatures, std::string stateDirectory, std::string resourceDirectory,
                Fetch fetch, ApplyTheme applyTheme);
    nlohmann::json catalog() const;
    std::string catalogState() const;
    std::string catalogError() const;
    std::string catalogURL() const;
    std::optional<std::string> installStatus(const std::string &identifier) const;
    void refreshCatalog(const std::function<void()> &changed);
    void installPackage(const nlohmann::json &package, const std::string &kind, const std::function<void()> &changed);
    std::string authorAvatar(const std::string &userID, size_t index) const;
    std::string pluginIconPath(const std::string &name);

  private:
    std::optional<std::string> fetchContent(const std::string &url, size_t maximum, std::string &failure) const;
    std::optional<std::string> resolveCatalogReference(const std::string &reference) const;
    std::optional<std::string> fetchPackagePath(const std::string &base, const std::string &path, size_t maximum,
                                                std::string &failure) const;
    std::optional<nlohmann::json> parseVerifiedCatalog(const std::string &contents, const std::string &signature) const;
    std::optional<nlohmann::json> downloadCatalog(const std::string &url, std::string &failure) const;
    std::optional<nlohmann::json> cachedCatalog() const;
    bool installTheme(const nlohmann::json &package, std::string &failure);
    bool installPlugin(const nlohmann::json &package, std::string &failure);
    bool validPackageIdentifier(const std::string &identifier) const;

    SettingsStore &settings_;
    Themes &themes_;
    Signatures &signatures_;
    std::string stateDirectory_;
    std::string resourceDirectory_;
    Fetch fetch_;
    ApplyTheme applyTheme_;
    mutable std::mutex mutex_;
    bool loading_ = false;
    std::string state_ = "idle";
    std::string error_;
    nlohmann::json catalog_ = nlohmann::json::object();
    std::map<std::string, std::string> installStatuses_;
};

}
