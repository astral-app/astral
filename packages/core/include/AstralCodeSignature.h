#pragma once

#include <cstdint>
#include <cstring>
#include <optional>
#include <string>
#include <vector>

#include "AstralRust.h"

extern "C" {
AstralRustString astral_entitlement_blobs(const char *executable, size_t length);
AstralRustString astral_entitlements_from_blob(const char *blob, size_t length);
}

namespace astral::codesign {
inline std::vector<std::string> entitlementBlobs(const std::string &executablePath)
{
    return rust::takeFrames(astral_entitlement_blobs(executablePath.data(), executablePath.size()));
}

inline std::optional<std::string> entitlementsFromBlob(const std::string &blob)
{
    return rust::takeIfPresent(astral_entitlements_from_blob(blob.data(), blob.size()));
}
} // namespace astral::codesign
