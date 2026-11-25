#pragma once

#include <cstdint>
#include <optional>
#include <utility>
#include <string>

#include <nlohmann/json.hpp>

namespace astral::discord {

void setDataDirectory(const std::string &directory);
std::optional<nlohmann::json> guild(const std::string &guildID);
std::optional<nlohmann::json> member(const std::string &guildID, const std::string &userID);
std::optional<std::string> memberNamed(const std::string &guildID, const std::string &name);
std::optional<std::string> channelGuild(const std::string &channelID);
std::optional<nlohmann::json> user(const std::string &userID);
std::optional<std::string> friendNickname(const std::string &userID);
std::optional<int> relationshipType(const std::string &userID);
std::optional<std::string> messageChannel(const std::string &messageID);
std::optional<nlohmann::json> message(const std::string &channelID, const std::string &messageID);
std::optional<std::pair<std::string, std::string>> messageContaining(const std::string &text);
std::optional<nlohmann::json> rawMessage(const std::string &channelID, const std::string &messageID);
std::optional<nlohmann::json> emoji(const std::string &emojiID);
nlohmann::json guilds();
std::optional<std::string> currentUser();
std::optional<uint64_t> permissions(const std::string &guildID, const std::string &userID, const std::optional<std::string> &channelID);

} // namespace astral::discord
