#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "AstralRust.h"

extern "C" {
typedef struct {
    const char *name;
    size_t nameLength;
    uint64_t jsonOffset;
    uint64_t jsonLength;
    const char *packed;
    size_t packedLength;
} AstralCachedStore;

uint64_t astral_settings_hash(const char *json, size_t length);
AstralRustString astral_encode_settings_cache(uint64_t jsonHash, uint64_t jsonSize, const AstralCachedStore *stores, size_t count);
int astral_decode_settings_cache(const char *cache, size_t length, uint64_t jsonHash, uint64_t jsonSize,
                                 void (*visit)(void *, const char *, size_t, uint64_t, uint64_t, const char *, size_t), void *context);
}

namespace astral {
struct CachedStore {
    std::string name;
    uint64_t jsonOffset = 0;
    uint64_t jsonLength = 0;
    std::string packed;
};

inline uint64_t settingsHash(std::string_view json)
{
    return astral_settings_hash(json.data(), json.size());
}

inline std::string encodeSettingsCache(uint64_t jsonHash, uint64_t jsonSize, const std::vector<CachedStore> &stores)
{
    std::vector<AstralCachedStore> bridged;
    bridged.reserve(stores.size());
    for (const CachedStore &store : stores)
        bridged.push_back(
            {store.name.data(), store.name.size(), store.jsonOffset, store.jsonLength, store.packed.data(), store.packed.size()});
    return rust::take(astral_encode_settings_cache(jsonHash, jsonSize, bridged.data(), bridged.size()));
}

inline std::optional<std::vector<CachedStore>> decodeSettingsCache(std::string_view cache, uint64_t jsonHash, uint64_t jsonSize)
{
    std::vector<CachedStore> stores;
    auto visit = [](void *context, const char *name, size_t nameLength, uint64_t offset, uint64_t length, const char *packed,
                    size_t packedLength) {
        static_cast<std::vector<CachedStore> *>(context)->push_back(
            {std::string(name, nameLength), offset, length, std::string(packed, packedLength)});
    };
    if (!astral_decode_settings_cache(cache.data(), cache.size(), jsonHash, jsonSize, visit, &stores)) return std::nullopt;
    return stores;
}
} // namespace astral
