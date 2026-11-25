#pragma once

#include <functional>
#include <map>
#include <optional>
#include <string>
#include <string_view>

#include "AstralRust.h"

extern "C" {
typedef void (*AstralDigest)(void *context, const char *data, size_t length, AstralRustString *digest);
int astral_signatures_verify(const char *key, size_t keyLength, const char *message, size_t messageLength, const char *signature,
                             size_t signatureLength);
int astral_signatures_files(const char *key, size_t keyLength, const char *document, size_t documentLength, AstralRustString *files);
int astral_signatures_directory(const char *key, size_t keyLength, const char *directory, size_t directoryLength, const char *main,
                                size_t mainLength, AstralDigest digest, void *context);
}

namespace astral {
class Signatures
{
  public:
    using Files = std::map<std::string, std::string>;
    using Digest = std::function<std::string(const std::string &)>;

    Signatures(const std::string &publicKey, Digest digest) : key_(publicKey), digest_(std::move(digest)) {}

    bool verify(std::string_view message, const std::string &signature) const
    {
        return astral_signatures_verify(key_.data(), key_.size(), message.data(), message.size(), signature.data(), signature.size());
    }

    std::optional<Files> files(const std::string &document) const
    {
        AstralRustString flat{};
        if (!astral_signatures_files(key_.data(), key_.size(), document.data(), document.size(), &flat)) return std::nullopt;
        std::vector<std::string> entries = rust::takeList(flat);
        Files files;
        for (size_t index = 0; index + 1 < entries.size(); index += 2) files.emplace(entries[index], entries[index + 1]);
        return files;
    }

    bool directory(const std::string &directory, const std::string &main) const
    {
        if (!digest_) return false;
        auto digest = [](void *context, const char *data, size_t length, AstralRustString *result) {
            std::string value = (*static_cast<const Digest *>(context))(std::string(data, length));
            *result = astral_rust_string_from(value.data(), value.size());
        };
        return astral_signatures_directory(key_.data(), key_.size(), directory.data(), directory.size(), main.data(), main.size(), digest,
                                           const_cast<Digest *>(&digest_));
    }

  private:
    std::string key_;
    Digest digest_;
};
} // namespace astral
