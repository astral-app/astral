#pragma once

#include <functional>
#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace astral {
class Signatures
{
  public:
    using Files = std::map<std::string, std::string>;
    using Digest = std::function<std::string(const std::string &)>;

    Signatures(const std::string &publicKey, Digest digest);
    bool verify(std::string_view message, const std::string &signature) const;
    std::optional<Files> files(const std::string &document) const;
    bool directory(const std::string &directory, const std::string &main) const;

  private:
    std::vector<uint8_t> key_;
    Digest digest_;
};
}
