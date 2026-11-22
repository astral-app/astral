#include "PluginStorage.h"

#include <initializer_list>
#include <sqlite3.h>

namespace astral {
namespace {
class Statement
{
  public:
    Statement(sqlite3 *database, const char *sql, std::initializer_list<std::string> values)
    {
        if (!database || sqlite3_prepare_v2(database, sql, -1, &statement, nullptr) != SQLITE_OK) {
            statement = nullptr;
            return;
        }
        int position = 1;
        for (const auto &value : values) sqlite3_bind_text(statement, position++, value.data(), (int)value.size(), SQLITE_TRANSIENT);
    }
    ~Statement() { sqlite3_finalize(statement); }
    Statement(const Statement &) = delete;
    Statement &operator=(const Statement &) = delete;

    int step() { return statement ? sqlite3_step(statement) : SQLITE_ERROR; }
    std::string text(int column) const
    {
        const unsigned char *value = sqlite3_column_text(statement, column);
        return value ? reinterpret_cast<const char *>(value) : "";
    }
    int64_t integer(int column) const { return sqlite3_column_int64(statement, column); }

  private:
    sqlite3_stmt *statement = nullptr;
};
} // namespace

PluginStorage::PluginStorage(const std::string &path, size_t limitPerPlugin) : limitPerPlugin(limitPerPlugin)
{
    if (sqlite3_open_v2(path.c_str(), &database, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX, nullptr) !=
        SQLITE_OK) {
        sqlite3_close(database);
        database = nullptr;
        return;
    }
    sqlite3_exec(
        database,
        "PRAGMA journal_mode=WAL;"
        "CREATE TABLE IF NOT EXISTS storage (plugin TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (plugin, key))",
        nullptr, nullptr, nullptr);
}

PluginStorage::~PluginStorage()
{
    sqlite3_close(database);
}

std::optional<nlohmann::json> PluginStorage::get(const std::string &plugin, const std::string &key) const
{
    Statement statement(database, "SELECT value FROM storage WHERE plugin = ? AND key = ?", {plugin, key});
    if (statement.step() != SQLITE_ROW) return std::nullopt;
    nlohmann::json value = nlohmann::json::parse(statement.text(0), nullptr, false);
    if (value.is_discarded()) return std::nullopt;
    return value;
}

size_t PluginStorage::bytesExcept(const std::string &plugin, const std::string &key) const
{
    Statement statement(database, "SELECT COALESCE(SUM(LENGTH(key) + LENGTH(value)), 0) FROM storage WHERE plugin = ? AND key != ?",
                        {plugin, key});
    return statement.step() == SQLITE_ROW ? (size_t)statement.integer(0) : 0;
}

StorageResult PluginStorage::set(const std::string &plugin, const std::string &key, const nlohmann::json &value)
{
    if (!database) return StorageResult::Unavailable;
    if (value.is_null()) return remove(plugin, key) ? StorageResult::Removed : StorageResult::Failed;
    std::string text = value.dump();
    if (bytesExcept(plugin, key) + key.size() + text.size() > limitPerPlugin) return StorageResult::OverLimit;
    Statement statement(
        database,
        "INSERT INTO storage (plugin, key, value) VALUES (?, ?, ?) ON CONFLICT (plugin, key) DO UPDATE SET value = excluded.value",
        {plugin, key, text});
    return statement.step() == SQLITE_DONE ? StorageResult::Saved : StorageResult::Failed;
}

bool PluginStorage::remove(const std::string &plugin, const std::string &key)
{
    Statement statement(database, "DELETE FROM storage WHERE plugin = ? AND key = ?", {plugin, key});
    return statement.step() == SQLITE_DONE;
}

std::vector<std::string> PluginStorage::keys(const std::string &plugin) const
{
    std::vector<std::string> keys;
    Statement statement(database, "SELECT key FROM storage WHERE plugin = ? ORDER BY key", {plugin});
    while (statement.step() == SQLITE_ROW) keys.push_back(statement.text(0));
    return keys;
}

bool PluginStorage::removeAll(const std::string &plugin)
{
    Statement statement(database, "DELETE FROM storage WHERE plugin = ?", {plugin});
    return statement.step() == SQLITE_DONE;
}
} // namespace astral
