#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include <nlohmann/json.hpp>

#include "AstralCodeBlocks.h"
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
    std::function<std::optional<int>(const std::string &user)> relationship;
    std::function<std::optional<std::string>(const std::string &message)> messageChannel;
    std::function<std::optional<nlohmann::json>(const std::string &channel, const std::string &message)> message;
    std::function<std::optional<uint64_t>(const std::string &guild, const std::string &user, const std::optional<std::string> &channel)>
        permissions;
    std::function<void(uint64_t request, const nlohmann::json &options)> networkRequest;
    std::function<void(uint64_t request)> cancelNetworkRequest;
    std::function<bool(uint64_t request, const std::string &url, const std::string &callbackURL)> webAuth;
    std::function<void(uint64_t request)> cancelWebAuth;
    std::function<void(const std::string &title, const std::string &message)> showAlert;
    std::function<bool(uint64_t request, const nlohmann::json &spec)> confirm;
    std::function<std::optional<nlohmann::json>(const std::string &channel, const std::string &message)> rawMessage;
    std::function<bool(uint64_t sheet, const nlohmann::json &spec)> openSheet;
    std::function<void(uint64_t sheet, const nlohmann::json &spec)> updateSheet;
    std::function<void(uint64_t sheet)> closeSheet;
    std::function<void(const std::string &title, const std::string &message)> showToast;
    std::function<bool(const std::string &strength)> triggerHaptic;
    std::function<bool(const std::string &value)> writeClipboard;
    std::function<std::optional<std::string>()> readClipboard;
    std::function<bool(const std::string &url)> openURL;
    std::function<bool(uint64_t message, bool animated, const std::string &align)> scrollToMessage;
    std::function<bool(bool focus)> focusComposer;
    std::function<bool(const std::string &message)> dispatchGateway;
    std::function<bool(const std::string &identifier, bool prefix)> pressNode;
    std::function<std::optional<std::string>(const nlohmann::json &spec)> renderImage;
    std::function<nlohmann::json()> environment;
    std::function<bool(const std::string &text, bool replace)> insertComposerText;
    std::function<bool(bool on)> keepAlive;
    std::function<bool(const std::string &text, const std::string &language, double rate)> speak;
    std::function<bool(const std::string &path, double volume)> playSound;
    std::function<void()> stopAudio;
    std::function<bool(const std::string &identifier, const std::string &title, const std::string &body, double delay,
                       const std::string &link)>
        notify;
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

struct HttpRequest {
    std::string method;
    std::string url;
    std::string path;
    std::vector<std::pair<std::string, std::string>> headers;
    std::string body;
};

struct HttpResponse {
    int status = 200;
    std::vector<std::pair<std::string, std::string>> headers;
    std::string body;
};

struct HttpDecision {
    bool changed = false;
    std::optional<HttpResponse> response;
};

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
void pressSettingsButton(const std::string &identifier, const std::string &key);
void resetSettings(const std::string &identifier);
void completeNetworkRequest(uint64_t request, const nlohmann::json &response);
void completeConfirm(uint64_t request, bool accepted);
void sheetEvent(uint64_t sheet, const std::string &action, const std::string &value, bool closed);
void completeWebAuth(uint64_t request, const std::optional<std::string> &url, const std::optional<std::string> &cookies,
                     const std::optional<std::string> &error);
void completeFilePick(uint64_t request, const std::optional<std::string> &name, const std::optional<std::string> &data,
                      const std::optional<std::string> &error);
void emitEvent(const std::string &event);
std::optional<std::string> processGatewayMessage(std::string_view message);
bool needsGatewayEvents();
bool wantsHttp(const std::string &method, const std::string &host, const std::string &path);
HttpDecision interceptHttpRequest(HttpRequest &request);
bool interceptHttpResponse(const HttpRequest &request, HttpResponse &response);
bool pressMessageAction(std::string_view identifier);
bool pressInsertedNode(std::string_view identifier);
void setMenuMessage(uint64_t channelID, uint64_t messageID);
struct MediaSave {
    std::string url;
    std::string kind;
    std::string channelID;
    std::string messageID;
};
bool hasMediaSaveHandlers();
uint64_t resolveMediaSave(const MediaSave &save, std::function<void(std::optional<std::string>)> done);
void cancelMediaSave(uint64_t id);
bool hasOutgoingMessageTransforms();
std::optional<std::string> transformOutgoingMessage(const std::string &channel, const std::string &operation, const std::string &content);
nlohmann::json composerStyles(std::string_view text);
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
int AstralLuaMessageText(uint64_t messageID, char *text, size_t textCapacity, char *note, size_t noteCapacity);
int AstralLuaSendTransform(uint64_t channelID, const char *text, char *rewritten, size_t capacity);
size_t AstralLuaMessageMenu(AstralMenuEntry *entries, size_t capacity);
size_t AstralLuaDecorate(const AstralDecorationContext *context, AstralTag *tags, size_t capacity);
int AstralLuaMessageBackground(const AstralMessageBackgroundContext *context, AstralMessageBackgroundStyle *style);
int AstralLuaCodeBlock(const AstralCodeBlockContext *context, AstralRustString *card);
int AstralLuaPatchNode(const AstralPatchTarget *target, AstralNodePatch *patch);
int AstralLuaImageRewrite(const char *uri, char *rewritten, size_t capacity);
int AstralLuaSpanTransform(uint64_t messageID, const uint8_t *spans, size_t length, uint8_t **transformed, size_t *transformedLength);
int AstralLuaInsertNodes(const AstralPatchTarget *target, AstralInsertNode *nodes, size_t capacity, int *atStart);
int AstralLuaRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity);
}
