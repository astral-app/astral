#pragma once

#include <functional>
#include <map>
#include <mutex>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

#include "AstralLuaHost.h"
#include "AstralRust.h"
#include "AstralSettings.h"

extern "C" {
typedef AstralRustString (*AstralDevHostCall)(const char *method, size_t methodLength, const char *arguments, size_t argumentsLength,
                                              AstralCallback callback);
void astral_dev_start(AstralDevHostCall host, const AstralSettingsHandle *settings, const char *version, size_t versionLength,
                      const char *stateDirectory, size_t stateDirectoryLength);
void astral_dev_append_log(const char *line, size_t length);
AstralRustString astral_dev_pairing_code(void);
}

namespace astral::dev {

struct Request {
    std::string method;
    std::string route;
    std::map<std::string, std::string> query;
    std::string body;
    nlohmann::json json;
};

struct Response {
    int status = 200;
    nlohmann::json body = nlohmann::json::object();
};

struct Hooks {
    std::function<bool()> enabled;
    std::function<void(const std::function<void()> &)> onMain;
    std::function<void()> changed;
    std::function<nlohmann::json()> status;
    std::function<std::optional<Response>(const Request &)> routes;
    std::string version;
    std::string stateDirectory;
    SettingsStore *settings = nullptr;
};

inline Hooks &hooks()
{
    static Hooks stored;
    return stored;
}

inline std::string text(const nlohmann::json &object, const char *key)
{
    auto field = object.find(key);
    return field != object.end() && field->is_string() ? field->get<std::string>() : "";
}

inline Request requestFrom(const nlohmann::json &arguments)
{
    Request request;
    request.method = text(arguments, "method");
    request.route = text(arguments, "route");
    request.body = text(arguments, "body");
    auto query = arguments.find("query");
    if (query != arguments.end() && query->is_object())
        for (auto entry = query->begin(); entry != query->end(); ++entry)
            if (entry.value().is_string()) request.query[entry.key()] = entry.value().get<std::string>();
    request.json = request.body.empty() ? nlohmann::json() : nlohmann::json::parse(request.body, nullptr, false);
    return request;
}

inline nlohmann::json argumentAt(const nlohmann::json &arguments, size_t index)
{
    return arguments.is_array() && index < arguments.size() ? arguments[index] : nlohmann::json();
}

inline std::string stringAt(const nlohmann::json &arguments, size_t index)
{
    auto value = argumentAt(arguments, index);
    return value.is_string() ? value.get<std::string>() : "";
}

inline nlohmann::json dispatch(const std::string &method, const nlohmann::json &arguments, AstralCallback callback)
{
    auto &current = hooks();
    auto reply = rust::adoptReply(callback);
    if (method == "enabled") return current.enabled && current.enabled();
    if (method == "onMain") {
        std::function<void()> work = [reply] {
            if (reply) reply(std::nullopt);
        };
        if (current.onMain)
            current.onMain(work);
        else
            work();
    }
    if (method == "changed" && current.changed) current.changed();
    if (method == "status") return current.status ? current.status() : nlohmann::json::object();
    if (method == "routes" && current.routes) {
        if (auto response = current.routes(requestFrom(arguments))) return {{"status", response->status}, {"body", response->body}};
    }
    if (method == "plugins") return luahost::plugins();
    if (method == "reload") luahost::reload(stringAt(arguments, 0));
    if (method == "eval") return luahost::eval(stringAt(arguments, 0), stringAt(arguments, 1));
    if (method == "setEnabled") {
        auto enabled = argumentAt(arguments, 1);
        luahost::setEnabled(stringAt(arguments, 0), enabled.is_boolean() && enabled.get<bool>());
    }
    return nullptr;
}

inline AstralRustString hostCall(const char *method, size_t methodLength, const char *arguments, size_t argumentsLength,
                                 AstralCallback callback)
{
    auto parsed = nlohmann::json::parse(std::string(arguments ? arguments : "", argumentsLength), nullptr, false);
    auto result = dispatch(std::string(method ? method : "", methodLength), parsed, callback);
    if (result.is_null()) return {nullptr, 0};
    return rust::give(result.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace));
}

inline void start(Hooks configured)
{
    static std::once_flag started;
    std::call_once(started, [&] {
        hooks() = std::move(configured);
        const auto &current = hooks();
        astral_dev_start(&hostCall, current.settings ? current.settings->handle() : nullptr, current.version.data(), current.version.size(),
                         current.stateDirectory.data(), current.stateDirectory.size());
    });
}

inline void appendLog(const std::string &line)
{
    astral_dev_append_log(line.data(), line.size());
}

inline std::string pairingCode()
{
    return rust::take(astral_dev_pairing_code());
}

} // namespace astral::dev
