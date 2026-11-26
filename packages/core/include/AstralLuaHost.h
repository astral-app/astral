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
#include "AstralRust.h"
#include "AstralSettings.h"
#include <map>

extern "C" {
typedef AstralRustString (*AstralLuaPlatformCall)(const char *method, size_t methodLength, const char *arguments, size_t argumentsLength);
void astral_lua_host_start(const AstralSettingsHandle *settings, AstralLuaPlatformCall platform, const char *configuration,
                           size_t configurationLength, int startEnabled);
void astral_lua_host_stop(void);
void astral_lua_host_rescan(void);
AstralRustString astral_lua_host_plugins(void);
void astral_lua_host_remove_storage(const char *identifier, size_t length);
void astral_lua_host_set_enabled(const char *identifier, size_t length, int enabled);
void astral_lua_host_set_switch(const char *identifier, size_t length, const char *key, size_t keyLength, int on);
void astral_lua_host_set_choice(const char *identifier, size_t length, const char *key, size_t keyLength, const char *value,
                                size_t valueLength);
void astral_lua_host_set_number(const char *identifier, size_t length, const char *key, size_t keyLength, double value);
void astral_lua_host_set_text(const char *identifier, size_t length, const char *key, size_t keyLength, const char *value,
                              size_t valueLength);
void astral_lua_host_press_settings_button(const char *identifier, size_t length, const char *key, size_t keyLength);
void astral_lua_host_reset_settings(const char *identifier, size_t length);
void astral_lua_host_complete_network_request(uint64_t request, const char *response, size_t length);
void astral_lua_host_complete_confirm(uint64_t request, int accepted);
void astral_lua_host_sheet_event(uint64_t sheet, const char *action, size_t actionLength, const char *value, size_t valueLength,
                                 int closed);
void astral_lua_host_complete_web_auth(uint64_t request, const char *url, size_t urlLength, int hasURL, const char *cookies,
                                       size_t cookiesLength, int hasCookies, const char *error, size_t errorLength, int hasError);
void astral_lua_host_complete_file_pick(uint64_t request, const char *name, size_t nameLength, int hasName, const char *data,
                                        size_t dataLength, int hasData, const char *error, size_t errorLength, int hasError);
void astral_lua_host_emit_event(const char *event, size_t length);
AstralRustString astral_lua_host_process_gateway_message(const char *message, size_t length);
int astral_lua_host_needs_gateway_events(void);
int astral_lua_host_wants_http(const char *method, size_t methodLength, const char *host, size_t hostLength, const char *path,
                               size_t pathLength);
AstralRustString astral_lua_host_intercept_request(const char *request, size_t length);
AstralRustString astral_lua_host_intercept_response(const char *request, size_t requestLength, const char *response, size_t responseLength);
int astral_lua_host_press_message_action(const char *identifier, size_t length);
int astral_lua_host_press_inserted_node(const char *identifier, size_t length);
void astral_lua_host_set_menu_message(uint64_t channel, uint64_t message);
int astral_lua_host_has_media_save_handlers(void);
uint64_t astral_lua_host_resolve_media_save(const char *save, size_t length, AstralCallback done);
void astral_lua_host_cancel_media_save(uint64_t id);
int astral_lua_host_has_outgoing_message_transforms(void);
AstralRustString astral_lua_host_transform_outgoing_message(const char *channel, size_t channelLength, const char *operation,
                                                            size_t operationLength, const char *content, size_t contentLength);
AstralRustString astral_lua_host_composer_styles(const char *text, size_t length);
AstralRustString astral_lua_host_eval(const char *identifier, size_t length, const char *code, size_t codeLength);
void astral_lua_host_reload(const char *identifier, size_t length);
void astral_lua_host_remove(const char *identifier, size_t length);
}

namespace astral {

struct PickedMedia {
    int64_t size = -1;
    double duration = 0;
    std::string type;
};

struct LuaHostPlatform {
    std::function<void(uint64_t request, const nlohmann::json &options)> networkRequest;
    std::function<void(uint64_t request)> cancelNetworkRequest;
    std::function<bool(uint64_t request, const std::string &url, const std::string &callbackURL)> webAuth;
    std::function<void(uint64_t request)> cancelWebAuth;
    std::function<void(const std::string &title, const std::string &message)> showAlert;
    std::function<bool(uint64_t request, const nlohmann::json &spec)> confirm;
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
    std::function<bool()> dismissSheets;
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
    std::function<std::optional<PickedMedia>(const std::string &slot)> pickedMedia;
    std::function<std::optional<std::string>(const std::string &slot)> pickedVideoURL;
    std::function<bool(const std::string &data, const std::string &name)> offerPick;
    std::function<std::optional<nlohmann::json>(const std::string &channel, const std::string &message, const std::string &emoji,
                                                bool burst, uint64_t count, bool &fresh)>
        reactors;
    std::function<bool(const std::string &directory, const std::string &main)> verifySignature;
    std::function<void(const std::string &identifier, const std::string &name, const std::string &error)> recordFailure;
    std::function<void(const std::string &line)> developerLog;
    std::function<void()> refreshDecorations;
    std::vector<std::string> preferredLanguages;
    std::string storagePath;
    std::string discordDirectory;
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

struct MediaSave {
    std::string url;
    std::string kind;
    std::string channelID;
    std::string messageID;
};

inline LuaHostPlatform &platform()
{
    static LuaHostPlatform stored;
    return stored;
}

inline nlohmann::json at(const nlohmann::json &arguments, size_t index)
{
    return arguments.is_array() && index < arguments.size() ? arguments[index] : nlohmann::json();
}

inline std::string textAt(const nlohmann::json &arguments, size_t index)
{
    auto value = at(arguments, index);
    return value.is_string() ? value.get<std::string>() : "";
}

inline double numberAt(const nlohmann::json &arguments, size_t index)
{
    auto value = at(arguments, index);
    return value.is_number() ? value.get<double>() : 0;
}

inline bool flagAt(const nlohmann::json &arguments, size_t index)
{
    auto value = at(arguments, index);
    return value.is_boolean() && value.get<bool>();
}

inline uint64_t identifierAt(const nlohmann::json &arguments, size_t index)
{
    auto value = at(arguments, index);
    return value.is_number_unsigned() ? value.get<uint64_t>() : value.is_number_integer() ? static_cast<uint64_t>(value.get<int64_t>()) : 0;
}

inline std::string encodeBase64(const std::string &bytes)
{
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string encoded;
    encoded.reserve((bytes.size() + 2) / 3 * 4);
    for (size_t index = 0; index < bytes.size(); index += 3) {
        uint32_t value = static_cast<uint8_t>(bytes[index]) << 16;
        if (index + 1 < bytes.size()) value |= static_cast<uint8_t>(bytes[index + 1]) << 8;
        if (index + 2 < bytes.size()) value |= static_cast<uint8_t>(bytes[index + 2]);
        encoded += alphabet[(value >> 18) & 63];
        encoded += alphabet[(value >> 12) & 63];
        encoded += index + 1 < bytes.size() ? alphabet[(value >> 6) & 63] : '=';
        encoded += index + 2 < bytes.size() ? alphabet[value & 63] : '=';
    }
    return encoded;
}

inline std::string decodeBase64(const std::string &text)
{
    std::string decoded;
    uint32_t buffer = 0;
    int bits = 0;
    for (char character : text) {
        int value = character >= 'A' && character <= 'Z'   ? character - 'A'
                    : character >= 'a' && character <= 'z' ? character - 'a' + 26
                    : character >= '0' && character <= '9' ? character - '0' + 52
                    : character == '+'                     ? 62
                    : character == '/'                     ? 63
                                                           : -1;
        if (value < 0) break;
        buffer = (buffer << 6) | static_cast<uint32_t>(value);
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            decoded.push_back(static_cast<char>((buffer >> bits) & 0xFF));
        }
    }
    return decoded;
}

template <typename Function, typename Call> nlohmann::json answer(const Function &function, const Call &call)
{
    if (!function) return {{"missing", true}};
    try {
        return {{"value", call()}};
    } catch (...) {
        return {{"threw", true}};
    }
}

template <typename Function> nlohmann::json perform(const Function &function, const std::function<void()> &call)
{
    return answer(function, [&] {
        call();
        return nlohmann::json();
    });
}

inline nlohmann::json has(const std::string &method)
{
    auto &host = platform();
    static const std::map<std::string, std::function<bool()>> known = {
        {"confirm", [&] { return static_cast<bool>(host.confirm); }},
        {"openSheet", [&] { return static_cast<bool>(host.openSheet); }},
        {"pickFile", [&] { return static_cast<bool>(host.pickFile); }},
        {"webAuth", [&] { return static_cast<bool>(host.webAuth); }},
        {"networkRequest", [&] { return static_cast<bool>(host.networkRequest); }},
        {"dispatchGateway", [&] { return static_cast<bool>(host.dispatchGateway); }},
    };
    auto found = known.find(method);
    return {{"value", found != known.end() && found->second()}};
}

inline nlohmann::json dispatch(const std::string &method, const nlohmann::json &arguments)
{
    auto &host = platform();
    if (method == "has") return has(textAt(arguments, 0));
    if (method == "showToast") return perform(host.showToast, [&] { host.showToast(textAt(arguments, 0), textAt(arguments, 1)); });
    if (method == "showAlert") return perform(host.showAlert, [&] { host.showAlert(textAt(arguments, 0), textAt(arguments, 1)); });
    if (method == "writeClipboard") return answer(host.writeClipboard, [&] { return host.writeClipboard(textAt(arguments, 0)); });
    if (method == "readClipboard")
        return answer(host.readClipboard, [&] {
            auto value = host.readClipboard();
            return value ? nlohmann::json(*value) : nlohmann::json();
        });
    if (method == "openURL") return answer(host.openURL, [&] { return host.openURL(textAt(arguments, 0)); });
    if (method == "triggerHaptic") return answer(host.triggerHaptic, [&] { return host.triggerHaptic(textAt(arguments, 0)); });
    if (method == "keepAlive") return answer(host.keepAlive, [&] { return host.keepAlive(flagAt(arguments, 0)); });
    if (method == "speak")
        return answer(host.speak, [&] { return host.speak(textAt(arguments, 0), textAt(arguments, 1), numberAt(arguments, 2)); });
    if (method == "playSound") return answer(host.playSound, [&] { return host.playSound(textAt(arguments, 0), numberAt(arguments, 1)); });
    if (method == "stopAudio") return perform(host.stopAudio, [&] { host.stopAudio(); });
    if (method == "notify")
        return answer(host.notify, [&] {
            return host.notify(textAt(arguments, 0), textAt(arguments, 1), textAt(arguments, 2), numberAt(arguments, 3),
                               textAt(arguments, 4));
        });
    if (method == "scrollToMessage")
        return answer(host.scrollToMessage,
                      [&] { return host.scrollToMessage(identifierAt(arguments, 0), flagAt(arguments, 1), textAt(arguments, 2)); });
    if (method == "focusComposer") return answer(host.focusComposer, [&] { return host.focusComposer(flagAt(arguments, 0)); });
    if (method == "dismissSheets") return answer(host.dismissSheets, [&] { return host.dismissSheets(); });
    if (method == "insertComposerText")
        return answer(host.insertComposerText, [&] { return host.insertComposerText(textAt(arguments, 0), flagAt(arguments, 1)); });
    if (method == "refreshDecorations") return perform(host.refreshDecorations, [&] { host.refreshDecorations(); });
    if (method == "environment") return answer(host.environment, [&] { return host.environment(); });
    if (method == "developerLog") return perform(host.developerLog, [&] { host.developerLog(textAt(arguments, 0)); });
    if (method == "recordFailure")
        return perform(host.recordFailure, [&] { host.recordFailure(textAt(arguments, 0), textAt(arguments, 1), textAt(arguments, 2)); });
    if (method == "verifySignature")
        return answer(host.verifySignature, [&] { return host.verifySignature(textAt(arguments, 0), textAt(arguments, 1)); });
    if (method == "networkRequest")
        return perform(host.networkRequest, [&] { host.networkRequest(identifierAt(arguments, 0), at(arguments, 1)); });
    if (method == "cancelNetworkRequest")
        return perform(host.cancelNetworkRequest, [&] { host.cancelNetworkRequest(identifierAt(arguments, 0)); });
    if (method == "confirm") return answer(host.confirm, [&] { return host.confirm(identifierAt(arguments, 0), at(arguments, 1)); });
    if (method == "openSheet") return answer(host.openSheet, [&] { return host.openSheet(identifierAt(arguments, 0), at(arguments, 1)); });
    if (method == "updateSheet") return perform(host.updateSheet, [&] { host.updateSheet(identifierAt(arguments, 0), at(arguments, 1)); });
    if (method == "closeSheet") return perform(host.closeSheet, [&] { host.closeSheet(identifierAt(arguments, 0)); });
    if (method == "pressNode") return answer(host.pressNode, [&] { return host.pressNode(textAt(arguments, 0), flagAt(arguments, 1)); });
    if (method == "renderImage")
        return answer(host.renderImage, [&] {
            auto png = host.renderImage(at(arguments, 0));
            return png ? nlohmann::json(encodeBase64(*png)) : nlohmann::json();
        });
    if (method == "pickFile")
        return answer(host.pickFile, [&] {
            std::vector<std::string> extensions;
            for (const auto &extension : at(arguments, 1))
                if (extension.is_string()) extensions.push_back(extension.get<std::string>());
            return host.pickFile(identifierAt(arguments, 0), extensions);
        });
    if (method == "cancelFilePick") return perform(host.cancelFilePick, [&] { host.cancelFilePick(identifierAt(arguments, 0)); });
    if (method == "webAuth")
        return answer(host.webAuth, [&] { return host.webAuth(identifierAt(arguments, 0), textAt(arguments, 1), textAt(arguments, 2)); });
    if (method == "cancelWebAuth") return perform(host.cancelWebAuth, [&] { host.cancelWebAuth(identifierAt(arguments, 0)); });
    if (method == "dispatchGateway") return answer(host.dispatchGateway, [&] { return host.dispatchGateway(textAt(arguments, 0)); });
    if (method == "pickedMedia")
        return answer(host.pickedMedia, [&] {
            auto media = host.pickedMedia(textAt(arguments, 0));
            return media ? nlohmann::json{{"size", media->size}, {"duration", media->duration}, {"type", media->type}} : nlohmann::json();
        });
    if (method == "pickedVideoURL")
        return answer(host.pickedVideoURL, [&] {
            auto url = host.pickedVideoURL(textAt(arguments, 0));
            return url ? nlohmann::json(*url) : nlohmann::json();
        });
    if (method == "offerPick")
        return answer(host.offerPick, [&] { return host.offerPick(decodeBase64(textAt(arguments, 0)), textAt(arguments, 1)); });
    if (method == "reactors")
        return answer(host.reactors, [&] {
            bool fresh = false;
            auto users = host.reactors(textAt(arguments, 0), textAt(arguments, 1), textAt(arguments, 2), flagAt(arguments, 3),
                                       identifierAt(arguments, 4), fresh);
            return nlohmann::json{{"users", users ? *users : nlohmann::json()}, {"fresh", fresh}};
        });
    return {{"missing", true}};
}

inline AstralRustString platformCall(const char *method, size_t methodLength, const char *arguments, size_t argumentsLength)
{
    auto parsed = nlohmann::json::parse(std::string(arguments ? arguments : "", argumentsLength), nullptr, false);
    auto reply = dispatch(std::string(method ? method : "", methodLength), parsed);
    return rust::give(reply.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace));
}

inline nlohmann::json parsed(AstralRustString string)
{
    return nlohmann::json::parse(rust::take(string), nullptr, false);
}

inline void start(const std::vector<std::string> &directories, SettingsStore *store, bool startEnabled, LuaHostPlatform configured)
{
    nlohmann::json configuration = {{"directories", directories},
                                    {"preferredLanguages", configured.preferredLanguages},
                                    {"storagePath", configured.storagePath},
                                    {"discordDirectory", configured.discordDirectory},
                                    {"bundledDirectory", configured.bundledDirectory}};
    platform() = std::move(configured);
    std::string text = configuration.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    astral_lua_host_start(store ? store->handle() : nullptr, &platformCall, text.data(), text.size(), startEnabled);
}

inline void stop()
{
    astral_lua_host_stop();
    platform() = {};
}

inline void rescan()
{
    astral_lua_host_rescan();
}

inline nlohmann::json plugins()
{
    auto list = parsed(astral_lua_host_plugins());
    return list.is_array() ? list : nlohmann::json::array();
}

inline void removeStorage(const std::string &identifier)
{
    astral_lua_host_remove_storage(identifier.data(), identifier.size());
}

inline void setEnabled(const std::string &identifier, bool enabled)
{
    astral_lua_host_set_enabled(identifier.data(), identifier.size(), enabled);
}

inline void setSwitch(const std::string &identifier, const std::string &key, bool on)
{
    astral_lua_host_set_switch(identifier.data(), identifier.size(), key.data(), key.size(), on);
}

inline void setChoice(const std::string &identifier, const std::string &key, const std::string &value)
{
    astral_lua_host_set_choice(identifier.data(), identifier.size(), key.data(), key.size(), value.data(), value.size());
}

inline void setNumber(const std::string &identifier, const std::string &key, double value)
{
    astral_lua_host_set_number(identifier.data(), identifier.size(), key.data(), key.size(), value);
}

inline void setText(const std::string &identifier, const std::string &key, const std::string &value)
{
    astral_lua_host_set_text(identifier.data(), identifier.size(), key.data(), key.size(), value.data(), value.size());
}

inline void pressSettingsButton(const std::string &identifier, const std::string &key)
{
    astral_lua_host_press_settings_button(identifier.data(), identifier.size(), key.data(), key.size());
}

inline void resetSettings(const std::string &identifier)
{
    astral_lua_host_reset_settings(identifier.data(), identifier.size());
}

inline void completeNetworkRequest(uint64_t request, const nlohmann::json &response)
{
    std::string text = response.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    astral_lua_host_complete_network_request(request, text.data(), text.size());
}

inline void completeConfirm(uint64_t request, bool accepted)
{
    astral_lua_host_complete_confirm(request, accepted);
}

inline void sheetEvent(uint64_t sheet, const std::string &action, const std::string &value, bool closed)
{
    astral_lua_host_sheet_event(sheet, action.data(), action.size(), value.data(), value.size(), closed);
}

inline void completeWebAuth(uint64_t request, const std::optional<std::string> &url, const std::optional<std::string> &cookies,
                            const std::optional<std::string> &error)
{
    astral_lua_host_complete_web_auth(request, url ? url->data() : nullptr, url ? url->size() : 0, url.has_value(),
                                      cookies ? cookies->data() : nullptr, cookies ? cookies->size() : 0, cookies.has_value(),
                                      error ? error->data() : nullptr, error ? error->size() : 0, error.has_value());
}

inline void completeFilePick(uint64_t request, const std::optional<std::string> &name, const std::optional<std::string> &data,
                             const std::optional<std::string> &error)
{
    astral_lua_host_complete_file_pick(request, name ? name->data() : nullptr, name ? name->size() : 0, name.has_value(),
                                       data ? data->data() : nullptr, data ? data->size() : 0, data.has_value(),
                                       error ? error->data() : nullptr, error ? error->size() : 0, error.has_value());
}

inline void emitEvent(const std::string &event)
{
    astral_lua_host_emit_event(event.data(), event.size());
}

inline std::optional<std::string> processGatewayMessage(std::string_view message)
{
    return rust::takeIfPresent(astral_lua_host_process_gateway_message(message.data(), message.size()));
}

inline bool needsGatewayEvents()
{
    return astral_lua_host_needs_gateway_events();
}

inline bool wantsHttp(const std::string &method, const std::string &host, const std::string &path)
{
    return astral_lua_host_wants_http(method.data(), method.size(), host.data(), host.size(), path.data(), path.size());
}

inline nlohmann::json headersJSON(const std::vector<std::pair<std::string, std::string>> &headers)
{
    nlohmann::json list = nlohmann::json::array();
    for (const auto &[name, value] : headers) list.push_back({name, value});
    return list;
}

inline std::vector<std::pair<std::string, std::string>> headersFrom(const nlohmann::json &list)
{
    std::vector<std::pair<std::string, std::string>> headers;
    if (!list.is_array()) return headers;
    for (const auto &pair : list)
        if (pair.is_array() && pair.size() == 2 && pair[0].is_string() && pair[1].is_string())
            headers.emplace_back(pair[0].get<std::string>(), pair[1].get<std::string>());
    return headers;
}

inline nlohmann::json requestJSON(const HttpRequest &request)
{
    return {{"method", request.method},
            {"url", request.url},
            {"path", request.path},
            {"headers", headersJSON(request.headers)},
            {"body_base64", encodeBase64(request.body)}};
}

inline HttpResponse responseFrom(const nlohmann::json &value)
{
    HttpResponse response;
    response.status = value.value("status", 200);
    response.headers = headersFrom(value.value("headers", nlohmann::json()));
    response.body = decodeBase64(value.value("body_base64", std::string()));
    return response;
}

inline HttpDecision interceptHttpRequest(HttpRequest &request)
{
    std::string text = requestJSON(request).dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    auto reply = parsed(astral_lua_host_intercept_request(text.data(), text.size()));
    HttpDecision decision;
    if (!reply.is_object()) return decision;
    decision.changed = reply.value("changed", false);
    if (decision.changed && reply.contains("request") && reply["request"].is_object()) {
        const auto &changed = reply["request"];
        request.method = changed.value("method", request.method);
        request.url = changed.value("url", request.url);
        request.path = changed.value("path", request.path);
        request.headers = headersFrom(changed.value("headers", nlohmann::json()));
        request.body = decodeBase64(changed.value("body_base64", std::string()));
    }
    if (reply.contains("response") && reply["response"].is_object()) decision.response = responseFrom(reply["response"]);
    return decision;
}

inline bool interceptHttpResponse(const HttpRequest &request, HttpResponse &response)
{
    std::string requestText = requestJSON(request).dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    nlohmann::json responseValue = {
        {"status", response.status}, {"headers", headersJSON(response.headers)}, {"body_base64", encodeBase64(response.body)}};
    std::string responseText = responseValue.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    auto reply = rust::takeIfPresent(
        astral_lua_host_intercept_response(requestText.data(), requestText.size(), responseText.data(), responseText.size()));
    if (!reply) return false;
    auto value = nlohmann::json::parse(*reply, nullptr, false);
    if (!value.is_object()) return false;
    response = responseFrom(value);
    return true;
}

inline bool pressMessageAction(std::string_view identifier)
{
    return astral_lua_host_press_message_action(identifier.data(), identifier.size());
}

inline bool pressInsertedNode(std::string_view identifier)
{
    return astral_lua_host_press_inserted_node(identifier.data(), identifier.size());
}

inline void setMenuMessage(uint64_t channelID, uint64_t messageID)
{
    astral_lua_host_set_menu_message(channelID, messageID);
}

inline bool hasMediaSaveHandlers()
{
    return astral_lua_host_has_media_save_handlers();
}

inline uint64_t resolveMediaSave(const MediaSave &save, std::function<void(std::optional<std::string>)> done)
{
    nlohmann::json value = {{"url", save.url}, {"kind", save.kind}, {"channelID", save.channelID}, {"messageID", save.messageID}};
    std::string text = value.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    return astral_lua_host_resolve_media_save(text.data(), text.size(), rust::replyCallback(std::move(done)));
}

inline void cancelMediaSave(uint64_t id)
{
    astral_lua_host_cancel_media_save(id);
}

inline bool hasOutgoingMessageTransforms()
{
    return astral_lua_host_has_outgoing_message_transforms();
}

inline std::optional<std::string> transformOutgoingMessage(const std::string &channel, const std::string &operation,
                                                           const std::string &content)
{
    return rust::takeIfPresent(astral_lua_host_transform_outgoing_message(channel.data(), channel.size(), operation.data(),
                                                                          operation.size(), content.data(), content.size()));
}

inline nlohmann::json composerStyles(std::string_view text)
{
    auto styles = parsed(astral_lua_host_composer_styles(text.data(), text.size()));
    return styles.is_array() ? styles : nlohmann::json::array();
}

inline std::string eval(const std::string &identifier, const std::string &code)
{
    return rust::take(astral_lua_host_eval(identifier.data(), identifier.size(), code.data(), code.size()));
}

inline void reload(const std::string &identifier)
{
    astral_lua_host_reload(identifier.data(), identifier.size());
}

inline void remove(const std::string &identifier)
{
    astral_lua_host_remove(identifier.data(), identifier.size());
}

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
