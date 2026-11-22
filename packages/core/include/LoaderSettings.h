#pragma once

#include <chrono>
#include <cstdint>
#include <functional>
#include <map>
#include <mutex>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <vector>

#include "LoaderSettingsCache.h"

namespace loader {
class SettingsStore
{
public:
    explicit SettingsStore(std::string path,
                           std::chrono::milliseconds diskRecheck = std::chrono::milliseconds(500));

    void load();
    bool readable() const;
    void reload();
    void refresh();
    bool save() const;
    void refreshCache();

    std::string serialize() const;
    nlohmann::json snapshot() const;
    bool replace(const std::string &json);

    std::optional<nlohmann::json> get(const std::string &store, const std::string &key) const;
    bool read(const std::string &store, const std::string &key,
              const std::function<void(const nlohmann::json &)> &reader) const;
    bool getBoolean(const std::string &store, const std::string &key, bool fallback) const;
    std::string getString(const std::string &store, const std::string &key,
                          const std::string &fallback) const;
    bool set(const std::string &store, const std::string &key, nlohmann::json value);
    void remove(const std::string &store, const std::string &key);
    bool apply(const nlohmann::json &changes);
    bool stage(const nlohmann::json &changes);
    bool flush();

    const std::string &path() const;

private:
    static constexpr size_t kCorruptBackupsKept = 3;

    struct FileStamp {
        bool exists      = false;
        int64_t modified = 0;
        uint64_t size    = 0;

        bool operator==(const FileStamp &other) const = default;
    };

    struct RawStore {
        std::string json;
        std::string packed;
    };

    void backupCorruptFile() const;
    void pruneCorruptBackups() const;
    std::string lastGoodPath() const;
    void keepLastGoodLocked(const std::string &contents) const;
    bool restoreLastGoodLocked() const;
    std::string cachePath() const;
    FileStamp stampOnDisk() const;
    void refreshLocked(bool beforeWrite = false) const;
    bool adoptLocked(const std::string &contents) const;
    void loadLocked() const;
    void resetLocked() const;
    void parseStoreLocked(const std::string &store) const;
    void parseAllLocked() const;
    std::string dumpLocked(std::vector<CachedStore> &layout) const;
    const nlohmann::json *findLocked(const std::string &store, const std::string &key) const;
    void setLocked(const std::string &store, const std::string &key, nlohmann::json value) const;
    void removeLocked(const std::string &store, const std::string &key) const;
    void applyLocked(const nlohmann::json &changes) const;
    void mergeExternalEditsLocked() const;

    std::string path_;
    mutable nlohmann::json data_ = nlohmann::json::object();
    mutable std::map<std::string, RawStore> raw_;
    mutable FileStamp stamp_;
    mutable uint64_t jsonHash_ = 0;
    mutable bool cacheStale_   = true;
    mutable bool dirty_        = false;
    mutable std::vector<nlohmann::json> journal_;
    mutable bool replaced_   = false;
    mutable bool unreadable_ = false;
    mutable std::mutex mutex_;
    std::chrono::milliseconds diskRecheck_;
    mutable std::chrono::steady_clock::time_point checkedAt_{};
};
}
