#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "AstralRust.h"

extern "C" {
typedef int (*AstralReactorsFetcher)(const char *url, size_t urlLength, const char *headers, size_t headersLength, AstralRustString *body);
void astral_reactors_set_fetcher(AstralReactorsFetcher fetcher);
void astral_reactors_invalidate(void);
void astral_discord_note_request(const char *host, size_t hostLength, const char *path, size_t pathLength, const char *headers,
                                 size_t headersLength);
AstralRustString astral_reactors_lookup(const char *channel, size_t channelLength, const char *message, size_t messageLength,
                                        const char *emoji, size_t emojiLength, int burst, uint64_t count, int *fresh);
}

namespace astral::reactors {

struct FetchResult {
    int status = 0;
    std::string body;
};

using Headers = std::vector<std::pair<std::string, std::string>>;
using Fetcher = std::function<FetchResult(const std::string &url, const Headers &headers)>;

inline Fetcher &currentFetcher()
{
    static Fetcher fetcher;
    return fetcher;
}

inline void setFetcher(Fetcher fetcher)
{
    currentFetcher() = std::move(fetcher);
    astral_reactors_set_fetcher([](const char *url, size_t urlLength, const char *headers, size_t headersLength, AstralRustString *body) {
        auto &fetch = currentFetcher();
        if (!fetch) return 0;
        Headers pairs;
        std::vector<std::string> frames = rust::takeFrames(astral_rust_string_from(headers, headersLength));
        for (size_t index = 0; index + 1 < frames.size(); index += 2) pairs.emplace_back(frames[index], frames[index + 1]);
        FetchResult result = fetch(std::string(url, urlLength), pairs);
        *body = astral_rust_string_from(result.body.data(), result.body.size());
        return result.status;
    });
}

inline std::optional<nlohmann::json> lookup(const std::string &channelID, const std::string &messageID, const std::string &emoji,
                                            bool burst, uint64_t count, bool &fresh)
{
    int current = 0;
    auto found = rust::takeIfPresent(astral_reactors_lookup(channelID.data(), channelID.size(), messageID.data(), messageID.size(),
                                                            emoji.data(), emoji.size(), burst, count, &current));
    fresh = current != 0;
    if (!found) return std::nullopt;
    auto parsed = nlohmann::json::parse(*found, nullptr, false);
    if (parsed.is_discarded()) return std::nullopt;
    return parsed;
}

inline void noteDiscordRequest(const std::string &host, const std::string &path, const Headers &headers)
{
    std::string frames;
    auto append = [&](const std::string &entry) {
        uint32_t length = static_cast<uint32_t>(entry.size());
        for (int shift = 0; shift < 32; shift += 8) frames.push_back(static_cast<char>((length >> shift) & 0xFF));
        frames += entry;
    };
    for (const auto &[name, value] : headers) {
        append(name);
        append(value);
    }
    astral_discord_note_request(host.data(), host.size(), path.data(), path.size(), frames.data(), frames.size());
}

} // namespace astral::reactors
