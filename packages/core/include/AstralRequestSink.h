#pragma once

#include <cstdint>
#include <string>
#include <utility>
#include <vector>

#include "AstralRust.h"

extern "C" {
uint16_t astral_sink_start(void);
AstralRustString astral_sink_url(void);
AstralRustString astral_sink_respond_with(int status, const char *headers, size_t headersLength, const char *body, size_t bodyLength);
}

namespace astral::sink {
inline uint16_t start()
{
    return astral_sink_start();
}

inline std::string url()
{
    return rust::take(astral_sink_url());
}

inline std::string respondWith(int status, const std::vector<std::pair<std::string, std::string>> &headers, const std::string &body)
{
    std::string joined;
    for (const auto &[name, value] : headers) joined.append(name).append(1, '\0').append(value).append(1, '\0');
    return rust::take(astral_sink_respond_with(status, joined.data(), joined.size(), body.data(), body.size()));
}
} // namespace astral::sink
