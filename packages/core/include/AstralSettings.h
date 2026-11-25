#pragma once

#include <chrono>
#include <functional>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>

#include "AstralRust.h"

extern "C" {
typedef struct AstralSettingsHandle AstralSettingsHandle;
AstralSettingsHandle *astral_settings_open(const char *path, size_t length, uint64_t recheckMs);
void astral_settings_close(AstralSettingsHandle *settings);
void astral_settings_load(const AstralSettingsHandle *settings);
int astral_settings_readable(const AstralSettingsHandle *settings);
void astral_settings_reload(const AstralSettingsHandle *settings);
void astral_settings_refresh(const AstralSettingsHandle *settings);
int astral_settings_save(const AstralSettingsHandle *settings);
int astral_settings_flush(const AstralSettingsHandle *settings);
AstralRustString astral_settings_snapshot(const AstralSettingsHandle *settings, int pretty);
int astral_settings_replace(const AstralSettingsHandle *settings, const char *json, size_t length);
int astral_settings_get(const AstralSettingsHandle *settings, const char *store, size_t storeLength, const char *key, size_t keyLength,
                        AstralRustString *value);
int astral_settings_get_boolean(const AstralSettingsHandle *settings, const char *store, size_t storeLength, const char *key,
                                size_t keyLength, int fallback);
int astral_settings_get_string(const AstralSettingsHandle *settings, const char *store, size_t storeLength, const char *key,
                               size_t keyLength, AstralRustString *value);
int astral_settings_set(const AstralSettingsHandle *settings, const char *store, size_t storeLength, const char *key, size_t keyLength,
                        const char *json, size_t jsonLength);
void astral_settings_remove(const AstralSettingsHandle *settings, const char *store, size_t storeLength, const char *key, size_t keyLength);
int astral_settings_stage(const AstralSettingsHandle *settings, const char *changes, size_t length);
}

namespace astral {
class SettingsStore
{
  public:
    explicit SettingsStore(std::string path, std::chrono::milliseconds diskRecheck = std::chrono::milliseconds(500))
        : path_(std::move(path)), handle_(astral_settings_open(path_.data(), path_.size(), static_cast<uint64_t>(diskRecheck.count())))
    {
    }

    ~SettingsStore() { astral_settings_close(handle_); }

    SettingsStore(const SettingsStore &) = delete;
    SettingsStore &operator=(const SettingsStore &) = delete;

    void load() { astral_settings_load(handle_); }
    bool readable() const { return astral_settings_readable(handle_); }
    void reload() { astral_settings_reload(handle_); }
    void refresh() { astral_settings_refresh(handle_); }
    bool save() const { return astral_settings_save(handle_); }
    bool flush() { return astral_settings_flush(handle_); }

    std::string serialize() const { return rust::take(astral_settings_snapshot(handle_, 1)); }

    nlohmann::json snapshot() const { return nlohmann::json::parse(rust::take(astral_settings_snapshot(handle_, 0)), nullptr, false); }

    bool replace(const std::string &json) { return astral_settings_replace(handle_, json.data(), json.size()); }

    std::optional<nlohmann::json> get(const std::string &store, const std::string &key) const
    {
        AstralRustString value{};
        if (!astral_settings_get(handle_, store.data(), store.size(), key.data(), key.size(), &value)) return std::nullopt;
        return nlohmann::json::parse(rust::take(value), nullptr, false);
    }

    bool read(const std::string &store, const std::string &key, const std::function<void(const nlohmann::json &)> &reader) const
    {
        std::optional<nlohmann::json> value = get(store, key);
        if (value) reader(*value);
        return value.has_value();
    }

    bool getBoolean(const std::string &store, const std::string &key, bool fallback) const
    {
        return astral_settings_get_boolean(handle_, store.data(), store.size(), key.data(), key.size(), fallback);
    }

    std::string getString(const std::string &store, const std::string &key, const std::string &fallback) const
    {
        AstralRustString value{};
        if (!astral_settings_get_string(handle_, store.data(), store.size(), key.data(), key.size(), &value)) return fallback;
        return rust::take(value);
    }

    bool set(const std::string &store, const std::string &key, const nlohmann::json &value)
    {
        std::string json = value.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
        return astral_settings_set(handle_, store.data(), store.size(), key.data(), key.size(), json.data(), json.size());
    }

    void remove(const std::string &store, const std::string &key)
    {
        astral_settings_remove(handle_, store.data(), store.size(), key.data(), key.size());
    }

    bool stage(const nlohmann::json &changes)
    {
        std::string json = changes.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
        return astral_settings_stage(handle_, json.data(), json.size());
    }

    bool apply(const nlohmann::json &changes) { return stage(changes) && save(); }

    const std::string &path() const { return path_; }
    const AstralSettingsHandle *handle() const { return handle_; }

  private:
    std::string path_;
    AstralSettingsHandle *handle_;
};
} // namespace astral
