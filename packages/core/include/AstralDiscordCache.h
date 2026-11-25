#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <utility>

#include <nlohmann/json.hpp>

#include "AstralRust.h"

extern "C" {
void astral_discord_set_data_directory(const char *directory, size_t length);
AstralRustString astral_discord_guild(const char *id, size_t length);
AstralRustString astral_discord_member(const char *guildID, size_t guildLength, const char *userID, size_t userLength);
AstralRustString astral_discord_member_named(const char *guildID, size_t guildLength, const char *name, size_t nameLength);
AstralRustString astral_discord_channel_guild(const char *id, size_t length);
AstralRustString astral_discord_user(const char *id, size_t length);
AstralRustString astral_discord_friend_nickname(const char *id, size_t length);
int astral_discord_relationship_type(const char *id, size_t length, int *found);
AstralRustString astral_discord_message_channel(const char *id, size_t length);
AstralRustString astral_discord_message(const char *channelID, size_t channelLength, const char *messageID, size_t messageLength);
AstralRustString astral_discord_message_containing(const char *text, size_t length);
AstralRustString astral_discord_raw_message(const char *channelID, size_t channelLength, const char *messageID, size_t messageLength);
AstralRustString astral_discord_emoji(const char *id, size_t length);
AstralRustString astral_discord_guilds();
AstralRustString astral_discord_current_user();
int astral_discord_permissions(const char *guildID, size_t guildLength, const char *userID, size_t userLength, const char *channelID,
                               size_t channelLength, uint64_t *granted);
}

namespace astral::discord {

inline std::optional<nlohmann::json> parsedRecord(AstralRustString string)
{
    auto text = rust::takeIfPresent(string);
    if (!text) return std::nullopt;
    auto value = nlohmann::json::parse(*text, nullptr, false);
    if (value.is_discarded()) return std::nullopt;
    return value;
}

inline void setDataDirectory(const std::string &directory)
{
    astral_discord_set_data_directory(directory.data(), directory.size());
}

inline std::optional<nlohmann::json> guild(const std::string &guildID)
{
    return parsedRecord(astral_discord_guild(guildID.data(), guildID.size()));
}

inline std::optional<nlohmann::json> member(const std::string &guildID, const std::string &userID)
{
    return parsedRecord(astral_discord_member(guildID.data(), guildID.size(), userID.data(), userID.size()));
}

inline std::optional<std::string> memberNamed(const std::string &guildID, const std::string &name)
{
    return rust::takeIfPresent(astral_discord_member_named(guildID.data(), guildID.size(), name.data(), name.size()));
}

inline std::optional<std::string> channelGuild(const std::string &channelID)
{
    return rust::takeIfPresent(astral_discord_channel_guild(channelID.data(), channelID.size()));
}

inline std::optional<nlohmann::json> user(const std::string &userID)
{
    return parsedRecord(astral_discord_user(userID.data(), userID.size()));
}

inline std::optional<std::string> friendNickname(const std::string &userID)
{
    return rust::takeIfPresent(astral_discord_friend_nickname(userID.data(), userID.size()));
}

inline std::optional<int> relationshipType(const std::string &userID)
{
    int found = 0;
    int type = astral_discord_relationship_type(userID.data(), userID.size(), &found);
    if (!found) return std::nullopt;
    return type;
}

inline std::optional<std::string> messageChannel(const std::string &messageID)
{
    return rust::takeIfPresent(astral_discord_message_channel(messageID.data(), messageID.size()));
}

inline std::optional<nlohmann::json> message(const std::string &channelID, const std::string &messageID)
{
    return parsedRecord(astral_discord_message(channelID.data(), channelID.size(), messageID.data(), messageID.size()));
}

inline std::optional<std::pair<std::string, std::string>> messageContaining(const std::string &text)
{
    auto string = astral_discord_message_containing(text.data(), text.size());
    if (!string.bytes) return std::nullopt;
    auto parts = rust::takeFrames(string);
    if (parts.size() != 2) return std::nullopt;
    return std::make_pair(parts[0], parts[1]);
}

inline std::optional<nlohmann::json> rawMessage(const std::string &channelID, const std::string &messageID)
{
    return parsedRecord(astral_discord_raw_message(channelID.data(), channelID.size(), messageID.data(), messageID.size()));
}

inline std::optional<nlohmann::json> emoji(const std::string &emojiID)
{
    return parsedRecord(astral_discord_emoji(emojiID.data(), emojiID.size()));
}

inline nlohmann::json guilds()
{
    return parsedRecord(astral_discord_guilds()).value_or(nlohmann::json::array());
}

inline std::optional<std::string> currentUser()
{
    return rust::takeIfPresent(astral_discord_current_user());
}

inline std::optional<uint64_t> permissions(const std::string &guildID, const std::string &userID,
                                           const std::optional<std::string> &channelID)
{
    uint64_t granted = 0;
    if (!astral_discord_permissions(guildID.data(), guildID.size(), userID.data(), userID.size(), channelID ? channelID->data() : nullptr,
                                    channelID ? channelID->size() : 0, &granted))
        return std::nullopt;
    return granted;
}

} // namespace astral::discord
