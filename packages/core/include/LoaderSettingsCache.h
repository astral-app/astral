#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace loader {

struct CachedStore {
    std::string name;
    uint64_t jsonOffset = 0;
    uint64_t jsonLength = 0;
    std::string packed;
};

uint64_t settingsHash(std::string_view json);

std::string encodeSettingsCache(uint64_t jsonHash, uint64_t jsonSize,
                                const std::vector<CachedStore> &stores);

std::optional<std::vector<CachedStore>> decodeSettingsCache(std::string_view cache,
                                                            uint64_t jsonHash, uint64_t jsonSize);
}
