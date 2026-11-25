#pragma once

#include <functional>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>

#include "AstralRust.h"
#include "AstralSettings.h"

extern "C" {
typedef int (*AstralMarketplaceFetch)(void *context, const char *url, size_t length, AstralRustString *body);
typedef void (*AstralMarketplaceInstalled)(void *context, int kind, const char *identifier, size_t length);
void astral_marketplace_configure(const AstralSettingsHandle *settings, const char *state, size_t stateLength, const char *resources,
                                  size_t resourcesLength, const char *key, size_t keyLength, AstralMarketplaceFetch fetch,
                                  void *fetchContext, AstralMarketplaceInstalled installed, void *installedContext);
AstralRustString astral_marketplace_catalog(void);
AstralRustString astral_marketplace_state(void);
AstralRustString astral_marketplace_error(void);
AstralRustString astral_marketplace_url(void);
AstralRustString astral_marketplace_install_status(const char *identifier, size_t length);
void astral_marketplace_refresh(void (*changed)(void *), void *context, void (*release)(void *));
void astral_marketplace_install(const char *package, size_t length, int kind, void (*changed)(void *), void *context,
                                void (*release)(void *));
AstralRustString astral_marketplace_stop_list_reason(const char *phoenix, size_t length);
}

namespace astral::marketplace {
using Fetch = std::function<int(const std::string &url, std::string &body)>;
using Installed = std::function<void(bool theme, const std::string &identifier)>;

inline Fetch &fetchHook()
{
    static Fetch hook;
    return hook;
}

inline Installed &installedHook()
{
    static Installed hook;
    return hook;
}

inline void configure(const SettingsStore &settings, const std::string &state, const std::string &resources, const std::string &key,
                      Fetch fetch, Installed installed)
{
    fetchHook() = std::move(fetch);
    installedHook() = std::move(installed);
    auto fetchCall = [](void *, const char *url, size_t length, AstralRustString *body) {
        std::string text;
        int status = fetchHook() ? fetchHook()(std::string(url, length), text) : 0;
        *body = astral_rust_string_from(text.data(), text.size());
        return status;
    };
    auto installedCall = [](void *, int kind, const char *identifier, size_t length) {
        if (installedHook()) installedHook()(kind == 1, std::string(identifier, length));
    };
    astral_marketplace_configure(settings.handle(), state.data(), state.size(), resources.data(), resources.size(), key.data(), key.size(),
                                 fetchCall, nullptr, installedCall, nullptr);
}

inline nlohmann::json catalog()
{
    nlohmann::json parsed = nlohmann::json::parse(rust::take(astral_marketplace_catalog()), nullptr, false);
    return parsed.is_object() ? parsed : nlohmann::json::object();
}

inline std::string state()
{
    return rust::take(astral_marketplace_state());
}

inline std::string error()
{
    return rust::take(astral_marketplace_error());
}

inline std::string url()
{
    return rust::take(astral_marketplace_url());
}

inline std::optional<std::string> installStatus(const std::string &identifier)
{
    return rust::takeIfPresent(astral_marketplace_install_status(identifier.data(), identifier.size()));
}

inline void notifyChanged(void *context)
{
    auto &changed = *static_cast<std::function<void()> *>(context);
    if (changed) changed();
}

inline void releaseChanged(void *context)
{
    delete static_cast<std::function<void()> *>(context);
}

inline void refresh(const std::function<void()> &changed)
{
    astral_marketplace_refresh(notifyChanged, new std::function<void()>(changed), releaseChanged);
}

inline void install(const nlohmann::json &package, const std::string &kind, const std::function<void()> &changed)
{
    std::string json = package.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    astral_marketplace_install(json.data(), json.size(), kind == "theme" ? 1 : 0, notifyChanged, new std::function<void()>(changed),
                               releaseChanged);
}

inline std::optional<std::string> stopListReason(const std::string &phoenix)
{
    return rust::takeIfPresent(astral_marketplace_stop_list_reason(phoenix.data(), phoenix.size()));
}
} // namespace astral::marketplace
