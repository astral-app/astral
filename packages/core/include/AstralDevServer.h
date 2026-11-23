#pragma once

#include <functional>
#include <map>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace astral {

class SettingsStore;

namespace dev {

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

void start(Hooks hooks);
void appendLog(const std::string &line);
std::string pairingCode();

} // namespace dev

} // namespace astral
