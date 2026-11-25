#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

#include "AstralRust.h"

extern "C" {
typedef int (*AstralReactorsFetcher)(const char *url, size_t urlLength, const char *token, size_t tokenLength, AstralRustString *body);
void astral_reactors_set_fetcher(AstralReactorsFetcher fetcher);
AstralRustString astral_reactors_lookup(const char *channel, size_t channelLength, const char *message, size_t messageLength,
                                        const char *emoji, size_t emojiLength, int burst, uint64_t count, int *fresh);
}

namespace astral::reactors {

struct FetchResult {
    int status = 0;
    std::string body;
};

using Fetcher = std::function<FetchResult(const std::string &url, const std::string &token)>;

inline Fetcher &currentFetcher()
{
    static Fetcher fetcher;
    return fetcher;
}

inline void setFetcher(Fetcher fetcher)
{
    currentFetcher() = std::move(fetcher);
    astral_reactors_set_fetcher([](const char *url, size_t urlLength, const char *token, size_t tokenLength, AstralRustString *body) {
        auto &fetch = currentFetcher();
        if (!fetch) return 0;
        FetchResult result = fetch(std::string(url, urlLength), std::string(token, tokenLength));
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

} // namespace astral::reactors
