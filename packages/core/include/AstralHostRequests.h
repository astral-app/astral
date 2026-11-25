#pragma once

#include <optional>
#include <string>
#include <vector>

#include "AstralLuaHost.h"
#include "AstralRequestSink.h"
#include "AstralRust.h"
#include "LuaLocale.h"

extern "C" {
AstralRustString astral_host_request_parse(const char *frame, size_t length);
AstralRustString astral_host_request_rebuild(const char *frame, size_t length, const char *request, size_t requestLength);
AstralRustString astral_url_host(const char *url, size_t length);
AstralRustString astral_url_path(const char *url, size_t length);
}

namespace astral::hostrequests {

inline std::optional<luahost::HttpRequest> parse(const std::vector<uint8_t> &frame)
{
    AstralRustString parsed = astral_host_request_parse(reinterpret_cast<const char *>(frame.data()), frame.size());
    if (!parsed.bytes) return std::nullopt;
    std::vector<std::string> frames = rust::takeFrames(parsed);
    if (frames.size() < 4 || frames.size() % 2 != 0) return std::nullopt;
    luahost::HttpRequest request{frames[0], frames[1], frames[2], {}, frames[3]};
    for (size_t index = 4; index + 1 < frames.size(); index += 2) request.headers.emplace_back(frames[index], frames[index + 1]);
    return request;
}

inline std::vector<uint8_t> rebuild(const std::vector<uint8_t> &frame, const luahost::HttpRequest &request)
{
    std::string frames;
    for (const std::string *part : {&request.method, &request.url, &request.path, &request.body}) lua::appendFrame(frames, *part);
    for (const auto &[name, value] : request.headers) {
        lua::appendFrame(frames, name);
        lua::appendFrame(frames, value);
    }
    std::string rebuilt =
        rust::take(astral_host_request_rebuild(reinterpret_cast<const char *>(frame.data()), frame.size(), frames.data(), frames.size()));
    return std::vector<uint8_t>(rebuilt.begin(), rebuilt.end());
}

inline std::string hostOf(const std::string &url)
{
    return rust::take(astral_url_host(url.data(), url.size()));
}

inline std::string pathOf(const std::string &url)
{
    return rust::take(astral_url_path(url.data(), url.size()));
}

inline bool wanted(const std::vector<uint8_t> &frame)
{
    auto request = parse(frame);
    return request && luahost::wantsHttp(request->method, hostOf(request->url), request->path);
}

inline std::optional<std::vector<uint8_t>> intercept(const std::vector<uint8_t> &frame)
{
    auto parsed = parse(frame);
    if (!parsed) return std::nullopt;
    luahost::HttpRequest request = *parsed;
    auto decision = luahost::interceptHttpRequest(request);
    if (decision.response) {
        std::string sink = astral::sink::respondWith(decision.response->status, decision.response->headers, decision.response->body);
        if (sink.empty()) return std::nullopt;
        luahost::HttpRequest answered = *parsed;
        answered.method = "GET";
        answered.url = sink;
        answered.path = pathOf(sink);
        answered.body.clear();
        std::vector<std::pair<std::string, std::string>> headers;
        for (const auto &[name, value] : answered.headers)
            if (name != "authorization" && name != "cookie" && name != "content-type") headers.emplace_back(name, value);
        answered.headers = std::move(headers);
        return rebuild(frame, answered);
    }
    if (!decision.changed) return std::nullopt;
    return rebuild(frame, request);
}

} // namespace astral::hostrequests
