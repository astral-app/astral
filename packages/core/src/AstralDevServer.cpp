#define TOML_EXCEPTIONS 0
#include "AstralDevServer.h"

#include <arpa/inet.h>
#include <chrono>
#include <deque>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <netinet/in.h>
#include <random>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>

#include <toml++/toml.hpp>

#include "AstralLuaHost.h"
#include "AstralSettings.h"

namespace {

constexpr uint16_t kPort = 9324;
constexpr size_t kMaxLog = 500;
constexpr size_t kMaxRequest = 512u << 10;
constexpr const char *kPairingAlphabet = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

std::mutex gLogMutex;
std::deque<nlohmann::json> gLog;
uint64_t gLogSequence = 0;
astral::dev::Hooks gHooks;
std::once_flag gStarted;

using astral::dev::Request;
using astral::dev::Response;

double now()
{
    return std::chrono::duration<double>(std::chrono::system_clock::now().time_since_epoch()).count();
}

void onMain(const std::function<void()> &work)
{
    if (gHooks.onMain)
        gHooks.onMain(work);
    else
        work();
}

void changed()
{
    if (gHooks.changed) gHooks.changed();
}

std::string lower(std::string value)
{
    for (auto &character : value) character = (char)std::tolower((unsigned char)character);
    return value;
}

std::string trim(const std::string &value)
{
    size_t start = value.find_first_not_of(" \t");
    size_t end = value.find_last_not_of(" \t\r");
    return start == std::string::npos ? std::string() : value.substr(start, end - start + 1);
}

std::string decode(const std::string &value)
{
    std::string decoded;
    for (size_t index = 0; index < value.size(); index++) {
        if (value[index] == '%' && index + 2 < value.size()) {
            decoded += (char)std::stoi(value.substr(index + 1, 2), nullptr, 16);
            index += 2;
        } else {
            decoded += value[index] == '+' ? ' ' : value[index];
        }
    }
    return decoded;
}

void splitPath(const std::string &path, Request &request)
{
    size_t question = path.find('?');
    request.route = path.substr(0, question);
    if (question == std::string::npos) return;
    std::string query = path.substr(question + 1);
    size_t start = 0;
    while (start <= query.size()) {
        size_t end = query.find('&', start);
        std::string pair = query.substr(start, end == std::string::npos ? std::string::npos : end - start);
        size_t equals = pair.find('=');
        if (!pair.empty())
            request.query[decode(pair.substr(0, equals))] = equals == std::string::npos ? "" : decode(pair.substr(equals + 1));
        if (end == std::string::npos) break;
        start = end + 1;
    }
}

bool pairingMatches(const std::string &offered)
{
    std::string expected = astral::dev::pairingCode();
    std::string given;
    for (char character : offered) given += (char)std::toupper((unsigned char)character);
    if (given.size() != expected.size()) return false;
    unsigned char difference = 0;
    for (size_t index = 0; index < expected.size(); index++) difference |= (unsigned char)(expected[index] ^ given[index]);
    return difference == 0;
}

bool safeRelative(const std::string &path)
{
    return !path.empty() && path[0] != '/' && path.find("..") == std::string::npos && path.find('\\') == std::string::npos;
}

void writeFile(const std::filesystem::path &path, const std::string &text)
{
    std::error_code error;
    std::filesystem::create_directories(path.parent_path(), error);
    std::ofstream(path, std::ios::binary | std::ios::trunc) << text;
}

Response putPlugin(const Request &request)
{
    const auto &body = request.json;
    if (!body.is_object() || !body.contains("manifest") || !body["manifest"].is_string() || !body.contains("main") ||
        !body["main"].is_string())
        return {400, {{"error", "manifest and main are required"}}};
    std::string manifestText = body["manifest"].get<std::string>();
    toml::parse_result manifest = toml::parse(manifestText);
    if (!manifest) return {400, {{"error", "invalid manifest"}}};
    const toml::table &table = manifest.table();
    std::string identifier = table["id"].value_or(std::string());
    std::string main = table["main"].value_or(std::string("main.lua"));
    if (identifier.empty() || identifier.find('/') != std::string::npos || identifier.find("..") != std::string::npos ||
        main.find('/') != std::string::npos)
        return {400, {{"error", "invalid manifest"}}};
    std::filesystem::path directory = std::filesystem::path(gHooks.stateDirectory) / "plugins" / identifier;
    writeFile(directory / "manifest.toml", manifestText);
    writeFile(directory / main, body["main"].get<std::string>());
    if (body.contains("modules") && body["modules"].is_object()) {
        for (auto module = body["modules"].begin(); module != body["modules"].end(); ++module) {
            const std::string &path = module.key();
            std::string extension = std::filesystem::path(path).extension().string();
            bool translation = path.rfind("i18n/", 0) == 0 && extension == ".toml";
            if (!module.value().is_string() || !(extension == ".lua" || translation) || !safeRelative(path)) continue;
            writeFile(directory / path, module.value().get<std::string>());
        }
    }
    std::error_code error;
    std::filesystem::remove(directory / "signature", error);
    onMain([&] {
        astral::luahost::reload(identifier);
        changed();
    });
    for (const auto &plugin : astral::luahost::plugins()) {
        if (plugin.value("id", "") == identifier)
            return {200, {{"id", identifier}, {"running", plugin.value("running", false)}, {"error", plugin.value("error", "")}}};
    }
    return {500, {{"error", "the plugin did not load"}}};
}

Response logs(const Request &request)
{
    uint64_t since = 0;
    auto entry = request.query.find("since");
    if (entry != request.query.end()) since = strtoull(entry->second.c_str(), nullptr, 10);
    nlohmann::json lines = nlohmann::json::array();
    std::lock_guard<std::mutex> lock(gLogMutex);
    for (const auto &line : gLog)
        if (line["id"].get<uint64_t>() > since) lines.push_back(line);
    return {200, lines};
}

Response status()
{
    nlohmann::json body = gHooks.status ? gHooks.status() : nlohmann::json::object();
    if (!body.is_object()) body = nlohmann::json::object();
    body["astral"] = gHooks.version;
    body["plugins"] = astral::luahost::plugins();
    return {200, body};
}

Response eval(const Request &request)
{
    const auto &body = request.json;
    if (!body.is_object() || !body.contains("plugin") || !body["plugin"].is_string() || !body.contains("code") || !body["code"].is_string())
        return {400, {{"error", "plugin and code are required"}}};
    return {200, {{"result", astral::luahost::eval(body["plugin"].get<std::string>(), body["code"].get<std::string>())}}};
}

Response setPluginEnabled(const Request &request)
{
    const auto &body = request.json;
    if (!body.is_object() || !body.contains("id") || !body["id"].is_string() || !body.contains("enabled") || !body["enabled"].is_boolean())
        return {400, {{"error", "id and enabled are required"}}};
    std::string identifier = body["id"].get<std::string>();
    bool enabled = body["enabled"].get<bool>();
    onMain([&] {
        astral::luahost::setEnabled(identifier, enabled);
        changed();
    });
    for (const auto &plugin : astral::luahost::plugins()) {
        if (plugin.value("id", "") == identifier)
            return {200,
                    {{"id", identifier},
                     {"enabled", plugin.value("enabled", false)},
                     {"running", plugin.value("running", false)},
                     {"error", plugin.value("error", "")}}};
    }
    return {404, {{"error", "no such plugin"}}};
}

Response handle(Request &request, const std::string &pairing)
{
    if (!gHooks.enabled || !gHooks.enabled()) return {403, {{"error", "turn on developer mode in astral's settings"}}};
    if (!pairingMatches(pairing))
        return {401, {{"error", "pairing code missing or wrong; run astral dev pair <code> with the code under Developer mode"}}};
    request.json = request.body.empty() ? nlohmann::json() : nlohmann::json::parse(request.body, nullptr, false);
    if (request.method == "GET" && request.route == "/status") return status();
    if (request.method == "GET" && request.route == "/logs") return logs(request);
    if (request.method == "PUT" && request.route == "/plugin") return putPlugin(request);
    if (request.method == "POST" && request.route == "/eval") return eval(request);
    if (request.method == "POST" && request.route == "/plugin/enabled") return setPluginEnabled(request);
    if (gHooks.routes) {
        if (auto response = gHooks.routes(request)) return *response;
    }
    return {404, {{"error", "unknown route"}}};
}

std::string serialize(const Response &response)
{
    std::string payload = response.body.dump();
    return "HTTP/1.1 " + std::to_string(response.status) + (response.status == 200 ? " OK" : " Error") +
           "\r\nContent-Type: application/json\r\nContent-Length: " + std::to_string(payload.size()) + "\r\nConnection: close\r\n\r\n" +
           payload;
}

void serve(int client)
{
    std::string received;
    char buffer[16384];
    size_t expected = SIZE_MAX, headerEnd = 0;
    Request request;
    std::string pairing;
    while (received.size() < kMaxRequest) {
        ssize_t count = recv(client, buffer, sizeof(buffer), 0);
        if (count <= 0) break;
        received.append(buffer, (size_t)count);
        if (!headerEnd) {
            size_t end = received.find("\r\n\r\n");
            if (end == std::string::npos) continue;
            headerEnd = end + 4;
            std::string head = received.substr(0, end);
            size_t lineEnd = head.find("\r\n");
            std::string first = head.substr(0, lineEnd);
            size_t space = first.find(' ');
            size_t second = space == std::string::npos ? std::string::npos : first.find(' ', space + 1);
            if (space != std::string::npos) {
                request.method = first.substr(0, space);
                splitPath(first.substr(space + 1, second == std::string::npos ? std::string::npos : second - space - 1), request);
            }
            size_t length = 0;
            size_t cursor = lineEnd == std::string::npos ? head.size() : lineEnd + 2;
            while (cursor < head.size()) {
                size_t next = head.find("\r\n", cursor);
                std::string line = head.substr(cursor, next == std::string::npos ? std::string::npos : next - cursor);
                std::string name = lower(line.substr(0, line.find(':')));
                std::string value = line.find(':') == std::string::npos ? "" : trim(line.substr(line.find(':') + 1));
                if (name == "content-length") length = strtoull(value.c_str(), nullptr, 10);
                if (name == "x-astral-pairing") pairing = value;
                if (next == std::string::npos) break;
                cursor = next + 2;
            }
            expected = headerEnd + length;
        }
        if (received.size() >= expected) break;
    }
    if (headerEnd && received.size() >= headerEnd) request.body = received.substr(headerEnd);
    Response response = received.size() >= kMaxRequest ? Response{413, {{"error", "request too large"}}} : handle(request, pairing);
    std::string reply = serialize(response);
    send(client, reply.data(), reply.size(), 0);
    close(client);
}

void listenLoop()
{
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    if (listener < 0) return;
    int reuse = 1;
    setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_port = htons(kPort);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (bind(listener, reinterpret_cast<sockaddr *>(&address), sizeof(address)) != 0 || listen(listener, 8) != 0) {
        close(listener);
        return;
    }
    while (true) {
        int client = accept(listener, nullptr, nullptr);
        if (client < 0) continue;
        std::thread(serve, client).detach();
    }
}

} // namespace

namespace astral::dev {

void appendLog(const std::string &line)
{
    std::lock_guard<std::mutex> lock(gLogMutex);
    gLog.push_back({{"id", ++gLogSequence}, {"time", now()}, {"line", line}});
    while (gLog.size() > kMaxLog) gLog.pop_front();
}

std::string pairingCode()
{
    static std::once_flag once;
    static std::string code;
    std::call_once(once, [] {
        std::string stored = gHooks.settings ? gHooks.settings->getString("astral", "dev-pairing-code", "") : "";
        if (stored.size() == 8) {
            code = stored;
        } else {
            std::random_device device;
            std::uniform_int_distribution<size_t> pick(0, 31);
            for (int index = 0; index < 8; index++) code += kPairingAlphabet[pick(device)];
            if (gHooks.settings && gHooks.settings->set("astral", "dev-pairing-code", code)) gHooks.settings->save();
        }
        std::filesystem::path file = std::filesystem::path(gHooks.stateDirectory) / "dev-pairing-code";
        std::ofstream(file, std::ios::trunc) << code;
        std::error_code error;
        std::filesystem::permissions(file, std::filesystem::perms::owner_read | std::filesystem::perms::owner_write,
                                     std::filesystem::perm_options::replace, error);
    });
    return code;
}

void start(Hooks hooks)
{
    std::call_once(gStarted, [&] {
        gHooks = std::move(hooks);
        pairingCode();
        std::thread(listenLoop).detach();
    });
}

} // namespace astral::dev
