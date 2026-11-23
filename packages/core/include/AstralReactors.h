#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace astral::reactors {

struct FetchResult {
    int status = 0;
    std::string body;
};

using Fetcher = std::function<FetchResult(const std::string &url, const std::string &token)>;

void setFetcher(Fetcher fetcher);
std::optional<nlohmann::json> lookup(const std::string &channelID, const std::string &messageID, const std::string &emoji, bool burst,
                                     uint64_t count, bool &fresh);

} // namespace astral::reactors
