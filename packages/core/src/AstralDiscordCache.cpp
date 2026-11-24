#include "AstralDiscordCache.h"

#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <map>
#include <mutex>
#include <set>
#include <sqlite3.h>
#include <utility>

namespace {

using Clock = std::chrono::steady_clock;

constexpr auto kFreshness = std::chrono::seconds(30);
constexpr uint64_t kAdministrator = 1ull << 3;
constexpr uint64_t kAllPermissions = UINT64_MAX >> 11;

std::recursive_mutex gMutex;
std::string gDataDirectory;
sqlite3 *gDatabase = nullptr;
std::map<std::string, std::pair<Clock::time_point, nlohmann::json>> gCache;

std::string newestStore()
{
    std::error_code error;
    std::string newest;
    std::filesystem::file_time_type newestTime{};
    for (const auto &account : std::filesystem::directory_iterator(gDataDirectory + "/store", error)) {
        auto path = account.path() / "cache.sqlite";
        auto modified = std::filesystem::last_write_time(path, error);
        if (error) {
            error.clear();
            continue;
        }
        if (newest.empty() || modified > newestTime) {
            newest = path.string();
            newestTime = modified;
        }
    }
    return newest;
}

sqlite3 *database()
{
    if (gDatabase) return gDatabase;
    std::string path = newestStore();
    if (path.empty()) return nullptr;
    std::string uri = "file:" + path + "?mode=ro";
    if (sqlite3_open_v2(uri.c_str(), &gDatabase, SQLITE_OPEN_READONLY | SQLITE_OPEN_URI | SQLITE_OPEN_NOMUTEX, nullptr) != SQLITE_OK) {
        sqlite3_close(gDatabase);
        gDatabase = nullptr;
        return nullptr;
    }
    sqlite3_busy_timeout(gDatabase, 50);
    gCache.clear();
    return gDatabase;
}

const nlohmann::json *cached(const std::string &key)
{
    auto entry = gCache.find(key);
    if (entry == gCache.end() || Clock::now() - entry->second.first > kFreshness) return nullptr;
    return &entry->second.second;
}

std::optional<nlohmann::json> remember(const std::string &key, std::optional<nlohmann::json> value)
{
    gCache[key] = {Clock::now(), value ? *value : nlohmann::json()};
    return value;
}

std::optional<nlohmann::json> fromCache(const nlohmann::json &value)
{
    if (value.is_null()) return std::nullopt;
    return value;
}

std::optional<std::string> textFromCache(const nlohmann::json &value)
{
    if (!value.is_string()) return std::nullopt;
    return value.get<std::string>();
}

std::optional<std::string> column(const char *sql, const std::string &argument)
{
    sqlite3 *handle = database();
    sqlite3_stmt *statement = nullptr;
    if (!handle || sqlite3_prepare_v2(handle, sql, -1, &statement, nullptr) != SQLITE_OK) return std::nullopt;
    sqlite3_bind_text(statement, 1, argument.c_str(), -1, SQLITE_TRANSIENT);
    std::optional<std::string> result;
    if (sqlite3_step(statement) == SQLITE_ROW) {
        const void *bytes = sqlite3_column_blob(statement, 0);
        int length = sqlite3_column_bytes(statement, 0);
        if (bytes && length > 0) result = std::string(static_cast<const char *>(bytes), (size_t)length);
    }
    sqlite3_finalize(statement);
    return result;
}

std::optional<nlohmann::json> parsed(const std::optional<std::string> &text, bool object = true)
{
    if (!text) return std::nullopt;
    auto value = nlohmann::json::parse(*text, nullptr, false);
    if (value.is_discarded() || (object ? !value.is_object() : !value.is_array())) return std::nullopt;
    return value;
}

bool snowflake(const std::string &value)
{
    if (value.size() < 15 || value.size() > 20) return false;
    for (char character : value)
        if (character < '0' || character > '9') return false;
    return true;
}

std::string key(const std::string &id)
{
    return std::string(20 - std::min<size_t>(20, id.size()), '0') + id;
}

std::string unpadded(const std::string &value)
{
    size_t start = 0;
    while (start + 1 < value.size() && value[start] == '0') start++;
    return value.substr(start);
}

std::string firstPart(const std::string &value)
{
    return value.substr(0, value.find(':'));
}

std::string lastPart(const std::string &value)
{
    auto colon = value.rfind(':');
    return colon == std::string::npos ? value : value.substr(colon + 1);
}

std::string stringField(const nlohmann::json &object, const char *name, const std::string &fallback = "")
{
    auto field = object.find(name);
    return field != object.end() && field->is_string() ? field->get<std::string>() : fallback;
}

void copyStrings(const nlohmann::json &from, nlohmann::json &to, std::initializer_list<const char *> fields)
{
    for (const char *field : fields) {
        auto value = from.find(field);
        if (value != from.end() && value->is_string()) to[field] = *value;
    }
}

nlohmann::json roleColors(const nlohmann::json &role)
{
    nlohmann::json stops = nlohmann::json::object();
    auto colors = role.find("colors");
    if (colors != role.end() && colors->is_object()) {
        for (const char *name : {"primary_color", "secondary_color", "tertiary_color"}) {
            auto value = colors->find(name);
            if (value != colors->end() && value->is_number()) stops[name] = *value;
        }
    }
    auto color = role.find("color");
    if (!stops.contains("primary_color") && color != role.end() && color->is_number()) stops["primary_color"] = *color;
    return stops;
}

nlohmann::json numberField(const nlohmann::json &object, const char *name)
{
    auto field = object.find(name);
    return field != object.end() && field->is_number() ? *field : nlohmann::json(0);
}

std::optional<std::string> memberWhere(const char *sql, const std::string &guildKey, const std::string &name)
{
    sqlite3 *handle = database();
    sqlite3_stmt *statement = nullptr;
    if (!handle || sqlite3_prepare_v2(handle, sql, -1, &statement, nullptr) != SQLITE_OK) return std::nullopt;
    std::string pattern = guildKey + ":%";
    sqlite3_bind_text(statement, 1, pattern.c_str(), -1, SQLITE_TRANSIENT);
    sqlite3_bind_text(statement, 2, name.c_str(), -1, SQLITE_TRANSIENT);
    std::optional<std::string> user;
    if (sqlite3_step(statement) == SQLITE_ROW) {
        const unsigned char *found = sqlite3_column_text(statement, 0);
        if (found) user = unpadded(lastPart(reinterpret_cast<const char *>(found)));
    }
    sqlite3_finalize(statement);
    if (user && !snowflake(*user)) return std::nullopt;
    return user;
}

uint64_t bits(const nlohmann::json &value)
{
    return value.is_string() ? strtoull(value.get<std::string>().c_str(), nullptr, 10) : 0;
}

std::optional<nlohmann::json> overwrites(const std::string &guildID, const std::string &channelID)
{
    std::string cacheKey = "overwrites:" + guildID + ":" + channelID;
    if (auto hit = cached(cacheKey)) return fromCache(*hit);
    return remember(cacheKey, parsed(column("SELECT v FROM channel_overwrites WHERE k = ?", key(guildID) + ":" + key(channelID)), false));
}

uint64_t applyOverwrites(uint64_t permissions, const nlohmann::json &list, const std::string &guildID, const nlohmann::json &member,
                         const std::string &userID)
{
    std::set<std::string> roles;
    for (const auto &role : member.value("roles", nlohmann::json::array()))
        if (role.is_string()) roles.insert(role.get<std::string>());
    uint64_t roleAllow = 0, roleDeny = 0;
    for (const auto &overwrite : list) {
        if (!overwrite.is_object()) continue;
        std::string id = stringField(overwrite, "id");
        if (id == userID) continue;
        if (id == guildID) {
            permissions =
                (permissions & ~bits(overwrite.value("deny", nlohmann::json()))) | bits(overwrite.value("allow", nlohmann::json()));
        } else if (roles.count(id)) {
            roleAllow |= bits(overwrite.value("allow", nlohmann::json()));
            roleDeny |= bits(overwrite.value("deny", nlohmann::json()));
        }
    }
    permissions = (permissions & ~roleDeny) | roleAllow;
    for (const auto &overwrite : list) {
        if (!overwrite.is_object() || stringField(overwrite, "id") != userID) continue;
        permissions = (permissions & ~bits(overwrite.value("deny", nlohmann::json()))) | bits(overwrite.value("allow", nlohmann::json()));
    }
    return permissions;
}

} // namespace

namespace astral::discord {

void setDataDirectory(const std::string &directory)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (gDataDirectory == directory) return;
    gDataDirectory = directory;
    if (gDatabase) sqlite3_close(gDatabase);
    gDatabase = nullptr;
    gCache.clear();
}

std::optional<nlohmann::json> guild(const std::string &guildID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(guildID)) return std::nullopt;
    std::string cacheKey = "guild:" + guildID;
    if (auto hit = cached(cacheKey)) return fromCache(*hit);
    auto record = parsed(column("SELECT v FROM guilds WHERE k = ?", key(guildID)));
    if (!record || !record->contains("properties") || !(*record)["properties"].is_object()) return remember(cacheKey, std::nullopt);
    const auto &properties = (*record)["properties"];
    nlohmann::json roles = nlohmann::json::object();
    for (const auto &role : record->value("roles", nlohmann::json::array())) {
        if (!role.is_object() || !role.contains("id") || !role["id"].is_string()) continue;
        nlohmann::json entry = {{"id", role["id"]},
                                {"name", stringField(role, "name")},
                                {"permissions", stringField(role, "permissions", "0")},
                                {"color", numberField(role, "color")},
                                {"position", numberField(role, "position")},
                                {"colors", roleColors(role)}};
        copyStrings(role, entry, {"icon", "unicode_emoji"});
        roles[role["id"].get<std::string>()] = entry;
    }
    return remember(cacheKey, nlohmann::json{{"id", guildID},
                                             {"name", stringField(properties, "name")},
                                             {"owner_id", stringField(properties, "owner_id")},
                                             {"roles", roles}});
}

std::optional<nlohmann::json> member(const std::string &guildID, const std::string &userID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(guildID) || !snowflake(userID)) return std::nullopt;
    std::string cacheKey = "member:" + guildID + ":" + userID;
    if (auto hit = cached(cacheKey)) return fromCache(*hit);
    auto record = parsed(column("SELECT v FROM members WHERE k = ?", key(guildID) + ":" + key(userID)));
    if (!record) return remember(cacheKey, std::nullopt);
    nlohmann::json roles = nlohmann::json::array();
    for (const auto &role : record->value("roles", nlohmann::json::array()))
        if (role.is_string()) roles.push_back(role);
    return remember(cacheKey, nlohmann::json{{"user_id", userID}, {"roles", roles}, {"nick", stringField(*record, "nick")}});
}

std::optional<std::string> memberNamed(const std::string &guildID, const std::string &name)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(guildID) || name.empty()) return std::nullopt;
    std::string cacheKey = "member-named:" + guildID + ":" + name;
    if (auto hit = cached(cacheKey)) return textFromCache(*hit);
    std::string guildKey = key(guildID);
    auto user = memberWhere("SELECT k FROM members WHERE k LIKE ? AND json_extract(v, '$.nick') = ? LIMIT 1", guildKey, name);
    if (!user) {
        user = memberWhere("SELECT m.k FROM members m JOIN users u ON u.k = substr(m.k, 22) WHERE m.k LIKE ? AND "
                           "(json_extract(u.v, '$.global_name') = ?2 OR json_extract(u.v, '$.username') = ?2) LIMIT 1",
                           guildKey, name);
    }
    remember(cacheKey, user ? std::optional<nlohmann::json>(*user) : std::nullopt);
    return user;
}

std::optional<std::string> channelGuild(const std::string &channelID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(channelID)) return std::nullopt;
    std::string cacheKey = "channel:" + channelID;
    if (auto hit = cached(cacheKey)) return textFromCache(*hit);
    auto row = column("SELECT k FROM channels WHERE k LIKE '%:' || ?", key(channelID));
    std::string guildID = row ? unpadded(firstPart(*row)) : "";
    if (!snowflake(guildID)) {
        remember(cacheKey, std::nullopt);
        return std::nullopt;
    }
    remember(cacheKey, nlohmann::json(guildID));
    return guildID;
}

std::optional<nlohmann::json> user(const std::string &userID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(userID)) return std::nullopt;
    std::string cacheKey = "user:" + userID;
    if (auto hit = cached(cacheKey)) return fromCache(*hit);
    auto record = parsed(column("SELECT v FROM users WHERE k = ?", key(userID)));
    if (!record) return remember(cacheKey, std::nullopt);
    nlohmann::json found = {{"id", userID}};
    copyStrings(*record, found, {"avatar", "username", "global_name"});
    return remember(cacheKey, found);
}

std::optional<std::string> friendNickname(const std::string &userID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(userID)) return std::nullopt;
    std::string cacheKey = "relationship:" + userID;
    if (auto hit = cached(cacheKey)) return textFromCache(*hit);
    auto record = parsed(column("SELECT v FROM relationships WHERE k = ?", key(userID)));
    std::string nickname = record ? stringField(*record, "nickname") : "";
    if (nickname.empty()) {
        remember(cacheKey, std::nullopt);
        return std::nullopt;
    }
    remember(cacheKey, nlohmann::json(nickname));
    return nickname;
}

std::optional<std::string> messageChannel(const std::string &messageID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(messageID)) return std::nullopt;
    std::string cacheKey = "message:" + messageID;
    if (auto hit = cached(cacheKey)) return textFromCache(*hit);
    auto row = column("SELECT k FROM messages WHERE k LIKE '%:' || ?", key(messageID));
    std::string channelID = row ? unpadded(firstPart(*row)) : "";
    if (!snowflake(channelID)) {
        remember(cacheKey, std::nullopt);
        return std::nullopt;
    }
    remember(cacheKey, nlohmann::json(channelID));
    return channelID;
}

std::optional<nlohmann::json> message(const std::string &channelID, const std::string &messageID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!snowflake(channelID) || !snowflake(messageID)) return std::nullopt;
    std::string cacheKey = "message-record:" + channelID + ":" + messageID;
    if (auto hit = cached(cacheKey)) return fromCache(*hit);
    auto record = parsed(column("SELECT v FROM messages WHERE k = ?", key(channelID) + ":" + key(messageID)));
    if (!record) return std::nullopt;
    nlohmann::json author = record->contains("author") && (*record)["author"].is_object() ? (*record)["author"] : nlohmann::json::object();
    nlohmann::json found = {{"id", messageID}, {"channel_id", channelID}};
    copyStrings(*record, found, {"content", "webhook_id"});
    nlohmann::json authorRecord = {{"id", stringField(author, "id")},
                                   {"bot", author.contains("bot") && author["bot"].is_boolean() && author["bot"].get<bool>()}};
    copyStrings(author, authorRecord, {"username", "global_name"});
    found["author"] = authorRecord;
    if (record->contains("type") && (*record)["type"].is_number()) found["type"] = (*record)["type"];
    nlohmann::json mentions = nlohmann::json::array();
    for (const auto &mentioned : record->value("mentions", nlohmann::json::array())) {
        if (!mentioned.is_object() || !mentioned.contains("id") || !mentioned["id"].is_string()) continue;
        nlohmann::json entry = {{"id", mentioned["id"]}};
        copyStrings(mentioned, entry, {"username", "global_name", "avatar"});
        if (mentioned.contains("member") && mentioned["member"].is_object()) {
            std::string nick = stringField(mentioned["member"], "nick");
            if (mentioned["member"].contains("nick") && mentioned["member"]["nick"].is_string()) entry["nick"] = nick;
        }
        mentions.push_back(entry);
    }
    found["mentions"] = mentions;
    nlohmann::json roles = nlohmann::json::array();
    for (const auto &role : record->value("mention_roles", nlohmann::json::array()))
        if (role.is_string()) roles.push_back(role);
    found["mention_roles"] = roles;
    if (record->contains("message_reference") && (*record)["message_reference"].is_object()) {
        const auto &reference = (*record)["message_reference"];
        if (reference.contains("message_id") && reference["message_id"].is_string() && reference.contains("channel_id") &&
            reference["channel_id"].is_string()) {
            found["reference"] = {{"message_id", reference["message_id"]}, {"channel_id", reference["channel_id"]}};
        }
    }
    return remember(cacheKey, found);
}

std::optional<uint64_t> permissions(const std::string &guildID, const std::string &userID, const std::optional<std::string> &channelID)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    auto server = guild(guildID);
    auto membership = member(guildID, userID);
    if (!server) return std::nullopt;
    if ((*server)["owner_id"] == userID) return kAllPermissions;
    if (!membership) return std::nullopt;
    const auto &roles = (*server)["roles"];
    auto rolePermissions = [&](const std::string &role) {
        return roles.contains(role) ? bits(roles[role].value("permissions", nlohmann::json())) : 0;
    };
    uint64_t granted = rolePermissions(guildID);
    for (const auto &role : (*membership)["roles"])
        if (role.is_string()) granted |= rolePermissions(role.get<std::string>());
    if (granted & kAdministrator) return kAllPermissions;
    if (channelID && snowflake(*channelID)) {
        if (auto list = overwrites(guildID, *channelID)) granted = applyOverwrites(granted, *list, guildID, *membership, userID);
    }
    return granted;
}

} // namespace astral::discord
