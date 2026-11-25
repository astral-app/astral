#pragma once

#include <cstddef>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <vector>

#include "AstralRust.h"

extern "C" {
struct AstralPluginStorage;
AstralPluginStorage *astral_plugin_storage_open(const char *path, size_t length, size_t limit);
void astral_plugin_storage_close(AstralPluginStorage *storage);
int astral_plugin_storage_available(const AstralPluginStorage *storage);
AstralRustString astral_plugin_storage_get(const AstralPluginStorage *storage, const char *plugin, size_t pluginLength, const char *key,
                                           size_t keyLength);
int astral_plugin_storage_set(const AstralPluginStorage *storage, const char *plugin, size_t pluginLength, const char *key,
                              size_t keyLength, const char *value, size_t valueLength);
int astral_plugin_storage_remove(const AstralPluginStorage *storage, const char *plugin, size_t pluginLength, const char *key,
                                 size_t keyLength);
AstralRustString astral_plugin_storage_keys(const AstralPluginStorage *storage, const char *plugin, size_t pluginLength);
int astral_plugin_storage_remove_all(const AstralPluginStorage *storage, const char *plugin, size_t pluginLength);
}

namespace astral {
enum class StorageResult { Saved, Removed, Unavailable, OverLimit, Failed };

class PluginStorage
{
  public:
    explicit PluginStorage(const std::string &path, size_t limitPerPlugin = 512u << 10)
        : handle(astral_plugin_storage_open(path.data(), path.size(), limitPerPlugin)), limitPerPlugin(limitPerPlugin)
    {
    }
    ~PluginStorage() { astral_plugin_storage_close(handle); }
    PluginStorage(const PluginStorage &) = delete;
    PluginStorage &operator=(const PluginStorage &) = delete;

    bool available() const { return astral_plugin_storage_available(handle) != 0; }
    size_t limit() const { return limitPerPlugin; }

    std::optional<nlohmann::json> get(const std::string &plugin, const std::string &key) const
    {
        auto text = rust::takeIfPresent(astral_plugin_storage_get(handle, plugin.data(), plugin.size(), key.data(), key.size()));
        if (!text) return std::nullopt;
        auto value = nlohmann::json::parse(*text, nullptr, false);
        if (value.is_discarded()) return std::nullopt;
        return value;
    }

    StorageResult set(const std::string &plugin, const std::string &key, const nlohmann::json &value)
    {
        auto text = value.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
        return static_cast<StorageResult>(
            astral_plugin_storage_set(handle, plugin.data(), plugin.size(), key.data(), key.size(), text.data(), text.size()));
    }

    bool remove(const std::string &plugin, const std::string &key)
    {
        return astral_plugin_storage_remove(handle, plugin.data(), plugin.size(), key.data(), key.size()) != 0;
    }

    std::vector<std::string> keys(const std::string &plugin) const
    {
        return rust::takeList(astral_plugin_storage_keys(handle, plugin.data(), plugin.size()));
    }

    bool removeAll(const std::string &plugin) { return astral_plugin_storage_remove_all(handle, plugin.data(), plugin.size()) != 0; }

  private:
    AstralPluginStorage *handle;
    size_t limitPerPlugin;
};
} // namespace astral
