#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include <nlohmann/json.hpp>

#include "PhoenixWire.h"

namespace astral {

class SettingsStore;

struct LuaHostPlatform {
    std::function<std::optional<nlohmann::json>(const std::string &guild)> guild;
    std::function<std::optional<nlohmann::json>(const std::string &guild, const std::string &user)> member;
    std::function<std::optional<std::string>(const std::string &guild, const std::string &name)> memberNamed;
    std::function<std::optional<std::string>(const std::string &channel)> channelGuild;
    std::function<std::optional<nlohmann::json>(const std::string &user)> user;
    std::function<std::optional<std::string>(const std::string &user)> friendNickname;
    std::function<std::optional<std::string>(const std::string &message)> messageChannel;
    std::function<std::optional<nlohmann::json>(const std::string &channel, const std::string &message)> message;
    std::function<std::optional<uint64_t>(const std::string &guild, const std::string &user, const std::optional<std::string> &channel)>
        permissions;
    std::function<void(uint64_t request, const nlohmann::json &options)> networkRequest;
    std::function<void(uint64_t request)> cancelNetworkRequest;
    std::function<bool(uint64_t request, const std::string &url, const std::string &callbackURL)> webAuth;
    std::function<void(uint64_t request)> cancelWebAuth;
    std::function<void(const std::string &title, const std::string &message)> showAlert;
    std::function<void(const std::string &title, const std::string &message)> showToast;
    std::function<bool(const std::string &strength)> triggerHaptic;
    std::function<bool(const std::string &value)> writeClipboard;
    std::function<bool(uint64_t request, const std::vector<std::string> &extensions)> pickFile;
    std::function<void(uint64_t request)> cancelFilePick;
    std::function<std::optional<nlohmann::json>(const std::string &channel, const std::string &message, const std::string &emoji,
                                                bool burst, uint64_t count, bool &fresh)>
        reactors;
    std::function<bool(const std::string &directory, const std::string &main)> verifySignature;
    std::function<void(const std::string &identifier, const std::string &name, const std::string &error)> recordFailure;
    std::function<void(const std::string &line)> developerLog;
    std::function<void()> refreshDecorations;
    std::vector<std::string> preferredLanguages;
    std::string storagePath;
    std::string bundledDirectory;
};

namespace luahost {

void start(const std::vector<std::string> &directories, SettingsStore *store, bool startEnabled, LuaHostPlatform platform);
void stop();
void rescan();
nlohmann::json plugins();
void removeStorage(const std::string &identifier);
void setEnabled(const std::string &identifier, bool enabled);
void setSwitch(const std::string &identifier, const std::string &key, bool on);
void setChoice(const std::string &identifier, const std::string &key, const std::string &value);
void setNumber(const std::string &identifier, const std::string &key, double value);
void setText(const std::string &identifier, const std::string &key, const std::string &value);
void resetSettings(const std::string &identifier);
void completeNetworkRequest(uint64_t request, const nlohmann::json &response);
void completeWebAuth(uint64_t request, const std::optional<std::string> &url, const std::optional<std::string> &cookies,
                     const std::optional<std::string> &error);
void completeFilePick(uint64_t request, const std::optional<std::string> &name, const std::optional<std::string> &data,
                      const std::optional<std::string> &error);
void emitEvent(const std::string &event);
std::optional<std::string> processGatewayMessage(std::string_view message);
bool pressMessageAction(std::string_view identifier);
bool hasOutgoingMessageTransforms();
std::optional<std::string> transformOutgoingMessage(const std::string &channel, const std::string &operation, const std::string &content);
std::string eval(const std::string &identifier, const std::string &code);
void reload(const std::string &identifier);
void remove(const std::string &identifier);

} // namespace luahost

} // namespace astral

extern "C" {
int AstralLuaReaction(const AstralReactionContext *context, AstralReactionDecoration *decoration);
int AstralLuaTyping(const AstralNameContext *context, AstralMentionDecoration *decoration);
int AstralLuaNameColor(const AstralNameContext *context, uint32_t *color);
int AstralLuaMessageTextColor(uint64_t messageID, uint64_t channelID, uint32_t *color);
int AstralLuaMention(const AstralMentionContext *context, AstralMentionDecoration *decoration);
size_t AstralLuaDecorate(const AstralDecorationContext *context, AstralTag *tags, size_t capacity);
int AstralLuaRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity);
}
