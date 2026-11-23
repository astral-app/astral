#include "AstralSignatures.h"

#include <algorithm>
#include <filesystem>
#include <cerrno>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include "AstralFiles.h"
#include "AstralTOML.h"

extern "C" {
#include "monocypher-ed25519.h"
}

namespace {
std::optional<std::string> readRegular(const std::string &path, size_t maximum)
{
    int file = open(path.c_str(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    if (file < 0) return std::nullopt;
    struct stat status{};
    if (fstat(file, &status) != 0 || !S_ISREG(status.st_mode) || status.st_size < 0 || static_cast<uint64_t>(status.st_size) > maximum) {
        close(file);
        return std::nullopt;
    }
    std::string result(maximum + 1, '\0');
    size_t used = 0;
    while (used < result.size()) {
        ssize_t count = read(file, result.data() + used, result.size() - used);
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) {
            close(file);
            return std::nullopt;
        }
        if (count == 0) break;
        used += static_cast<size_t>(count);
    }
    close(file);
    if (used > maximum) return std::nullopt;
    result.resize(used);
    return result;
}

std::vector<uint8_t> base64(std::string text)
{
    const std::string alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    auto first = text.find_first_not_of(" \t\r\n");
    if (first == std::string::npos) return {};
    text = text.substr(first, text.find_last_not_of(" \t\r\n") - first + 1);
    if (text.empty() || text.size() % 4 != 0 || text.size() > 128) return {};
    std::vector<uint8_t> bytes;
    for (size_t position = 0; position < text.size(); position += 4) {
        uint32_t value = 0;
        int padding = 0;
        for (size_t digit = 0; digit < 4; digit++) {
            char character = text[position + digit];
            if (character == '=') {
                if (position + 4 != text.size() || digit < 2) return {};
                padding++;
                value <<= 6;
            } else {
                auto index = alphabet.find(character);
                if (padding || index == std::string::npos) return {};
                value = (value << 6) | static_cast<uint32_t>(index);
            }
        }
        if (padding > 2 || (padding == 2 && (value & 0xFFFF)) || (padding == 1 && (value & 0xFF))) return {};
        bytes.push_back(static_cast<uint8_t>(value >> 16));
        if (padding < 2) bytes.push_back(static_cast<uint8_t>(value >> 8));
        if (padding < 1) bytes.push_back(static_cast<uint8_t>(value));
    }
    return bytes;
}

bool safePath(const std::string &path)
{
    if (path.empty() || path.size() > 240 || path.find_first_of("\\\0", 0, 2) != std::string::npos) return false;
    std::filesystem::path relative(path);
    if (relative.is_absolute() || relative.lexically_normal().string() != path) return false;
    for (const auto &component : relative)
        if (component == ".." || component == ".") return false;
    return true;
}
}

astral::Signatures::Signatures(const std::string &publicKey, Digest digest) : key_(base64(publicKey)), digest_(std::move(digest)) {}

bool astral::Signatures::verify(std::string_view message, const std::string &signature) const
{
    auto bytes = base64(signature);
    return key_.size() == 32 && bytes.size() == 64 &&
           crypto_ed25519_check(bytes.data(), key_.data(), reinterpret_cast<const uint8_t *>(message.data()), message.size()) == 0;
}

std::optional<astral::Signatures::Files> astral::Signatures::files(const std::string &document) const
{
    if (document.size() > (128u << 10)) return std::nullopt;
    auto parsed = parseTOML(document);
    if (!parsed.is_object() || !parsed.contains("files") || !parsed["files"].is_object() || !parsed.contains("signature") ||
        !parsed["signature"].is_string() || parsed["files"].empty() || parsed["files"].size() > 512)
        return std::nullopt;
    Files files;
    for (const auto &[path, value] : parsed["files"].items()) {
        if (!safePath(path) || !value.is_string()) return std::nullopt;
        auto digest = value.get<std::string>();
        if (digest.size() != 64 || digest.find_first_not_of("0123456789abcdef") != std::string::npos) return std::nullopt;
        files.emplace(path, std::move(digest));
    }
    std::string message = "astral-plugin-v2\n";
    for (const auto &[path, digest] : files) message += path + " " + digest + "\n";
    return verify(message, parsed["signature"].get<std::string>()) ? std::optional<Files>(std::move(files)) : std::nullopt;
}

bool astral::Signatures::directory(const std::string &directory, const std::string &main) const
{
    if (!digest_ || !safePath(main)) return false;
    auto signaturePath = resolveInsideDirectory(directory, "signature");
    auto document = signaturePath ? readRegular(*signaturePath, 128u << 10) : std::nullopt;
    auto signedFiles = document ? files(*document) : std::nullopt;
    if (!signedFiles || !signedFiles->count("manifest.toml") || !signedFiles->count(main)) return false;
    size_t total = 0;
    for (const auto &[relative, digest] : *signedFiles) {
        auto path = resolveInsideDirectory(directory, relative);
        auto contents = path ? readRegular(*path, 1u << 20) : std::nullopt;
        if (!contents || contents->size() > (1u << 20) || (total += contents->size()) > (2u << 20) || digest_(*contents) != digest)
            return false;
    }
    std::error_code error;
    std::filesystem::recursive_directory_iterator cursor(directory, error), end;
    if (error) return false;
    size_t entries = 0;
    while (cursor != end) {
        if (++entries > 2048 || cursor->is_symlink(error) || error) return false;
        auto relative = cursor->path().lexically_relative(directory);
        if (relative == "tests")
            cursor.disable_recursion_pending();
        else if (cursor->is_regular_file(error)) {
            bool lua = relative.extension() == ".lua";
            bool translation = *relative.begin() == "i18n";
            if ((lua || translation) && !signedFiles->count(relative.generic_string())) return false;
        }
        if (error) return false;
        cursor.increment(error);
        if (error) return false;
    }
    return true;
}
