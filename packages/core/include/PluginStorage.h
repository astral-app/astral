#pragma once

#include <cstddef>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <vector>

struct sqlite3;

namespace loader {
enum class StorageResult { Saved, Removed, Unavailable, OverLimit, Failed };

class PluginStorage
{
  public:
    explicit PluginStorage(const std::string &path, size_t limitPerPlugin = 512u << 10);
    ~PluginStorage();
    PluginStorage(const PluginStorage &) = delete;
    PluginStorage &operator=(const PluginStorage &) = delete;

    bool available() const { return database != nullptr; }
    size_t limit() const { return limitPerPlugin; }
    std::optional<nlohmann::json> get(const std::string &plugin, const std::string &key) const;
    StorageResult set(const std::string &plugin, const std::string &key, const nlohmann::json &value);
    bool remove(const std::string &plugin, const std::string &key);
    std::vector<std::string> keys(const std::string &plugin) const;
    bool removeAll(const std::string &plugin);

  private:
    size_t bytesExcept(const std::string &plugin, const std::string &key) const;

    sqlite3 *database = nullptr;
    size_t limitPerPlugin;
};
} // namespace loader
