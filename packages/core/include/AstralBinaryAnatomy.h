#pragma once

#include <optional>
#include <string>

#include "AstralRust.h"

extern "C" int astral_binary_lines(const char *executable, size_t executableLength, const char *bundle, size_t bundleLength,
                                   AstralRustString *binary, AstralRustString *signer);

namespace astral::binary {
struct BinaryLines {
    std::string binary;
    std::optional<std::string> signer;
};

inline std::optional<BinaryLines> describeExecutable(const std::string &executablePath, const std::string &bundlePath)
{
    AstralRustString binary{}, signer{};
    if (!astral_binary_lines(executablePath.data(), executablePath.size(), bundlePath.data(), bundlePath.size(), &binary, &signer))
        return std::nullopt;
    return BinaryLines{rust::take(binary), rust::takeIfPresent(signer)};
}
} // namespace astral::binary
