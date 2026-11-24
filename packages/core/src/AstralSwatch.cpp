#include "AstralSwatch.h"

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <map>
#include <mutex>
#include <vector>

namespace astral::settings {
namespace {

constexpr int kSwatchSize = 48;
constexpr double kRingWidth = 2;
constexpr uint32_t kRingColor = 0x55808080;

uint32_t crc32(const uint8_t *bytes, size_t length, uint32_t crc = 0)
{
    crc = ~crc;
    for (size_t index = 0; index < length; index++) {
        crc ^= bytes[index];
        for (int bit = 0; bit < 8; bit++) crc = (crc >> 1) ^ (0xEDB88320u & (0u - (crc & 1)));
    }
    return ~crc;
}

void putBig32(std::vector<uint8_t> &out, uint32_t value)
{
    for (int shift = 24; shift >= 0; shift -= 8) out.push_back((uint8_t)(value >> shift));
}

void putChunk(std::vector<uint8_t> &out, const char *type, const std::vector<uint8_t> &data)
{
    putBig32(out, (uint32_t)data.size());
    size_t start = out.size();
    out.insert(out.end(), type, type + 4);
    out.insert(out.end(), data.begin(), data.end());
    putBig32(out, crc32(out.data() + start, out.size() - start));
}

std::vector<uint8_t> storedZlib(const std::vector<uint8_t> &raw)
{
    std::vector<uint8_t> out = {0x78, 0x01};
    uint32_t a = 1, b = 0;
    for (uint8_t byte : raw) {
        a = (a + byte) % 65521;
        b = (b + a) % 65521;
    }
    for (size_t offset = 0; offset < raw.size() || raw.empty(); offset += 65535) {
        size_t length = std::min<size_t>(65535, raw.size() - offset);
        out.push_back(offset + length >= raw.size() ? 1 : 0);
        out.push_back((uint8_t)length);
        out.push_back((uint8_t)(length >> 8));
        out.push_back((uint8_t)~length);
        out.push_back((uint8_t)(~length >> 8));
        out.insert(out.end(), raw.begin() + (long)offset, raw.begin() + (long)(offset + length));
        if (raw.empty()) break;
    }
    putBig32(out, (b << 16) | a);
    return out;
}

std::array<double, 4> unpack(uint32_t argb)
{
    return {(double)((argb >> 16) & 0xFF), (double)((argb >> 8) & 0xFF), (double)(argb & 0xFF), (double)(argb >> 24) / 255.0};
}

std::array<double, 4> over(const std::array<double, 4> &top, double topCoverage, const std::array<double, 4> &bottom)
{
    double alpha = top[3] * topCoverage;
    double outAlpha = alpha + bottom[3] * (1 - alpha);
    if (outAlpha <= 0) return {0, 0, 0, 0};
    std::array<double, 4> result{};
    for (int channel = 0; channel < 3; channel++)
        result[channel] = (top[channel] * alpha + bottom[channel] * bottom[3] * (1 - alpha)) / outAlpha;
    result[3] = outAlpha;
    return result;
}

std::vector<uint8_t> swatchPixels(uint32_t argb)
{
    std::vector<uint8_t> raw;
    raw.reserve((size_t)kSwatchSize * (kSwatchSize * 4 + 1));
    double center = kSwatchSize / 2.0, radius = center - 1;
    auto fill = unpack(argb), ring = unpack(kRingColor);
    for (int y = 0; y < kSwatchSize; y++) {
        raw.push_back(0);
        for (int x = 0; x < kSwatchSize; x++) {
            double distance = std::hypot(x + 0.5 - center, y + 0.5 - center);
            double disc = std::clamp(radius - distance + 0.5, 0.0, 1.0);
            double inner = std::clamp(radius - kRingWidth - distance + 0.5, 0.0, 1.0);
            auto pixel = over(fill, inner, over(ring, disc - inner, {0, 0, 0, 0}));
            for (int channel = 0; channel < 3; channel++) raw.push_back((uint8_t)std::lround(pixel[channel]));
            raw.push_back((uint8_t)std::lround(pixel[3] * 255));
        }
    }
    return raw;
}

std::string base64(const std::vector<uint8_t> &bytes)
{
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string out;
    for (size_t index = 0; index < bytes.size(); index += 3) {
        uint32_t chunk = (uint32_t)bytes[index] << 16;
        if (index + 1 < bytes.size()) chunk |= (uint32_t)bytes[index + 1] << 8;
        if (index + 2 < bytes.size()) chunk |= bytes[index + 2];
        out += alphabet[(chunk >> 18) & 63];
        out += alphabet[(chunk >> 12) & 63];
        out += index + 1 < bytes.size() ? alphabet[(chunk >> 6) & 63] : '=';
        out += index + 2 < bytes.size() ? alphabet[chunk & 63] : '=';
    }
    return out;
}

std::string encodeSwatch(uint32_t argb)
{
    std::vector<uint8_t> png = {0x89, 'P', 'N', 'G', '\r', '\n', 0x1A, '\n'};
    std::vector<uint8_t> header;
    putBig32(header, kSwatchSize);
    putBig32(header, kSwatchSize);
    header.insert(header.end(), {8, 6, 0, 0, 0});
    putChunk(png, "IHDR", header);
    putChunk(png, "IDAT", storedZlib(swatchPixels(argb)));
    putChunk(png, "IEND", {});
    return "data:image/png;base64," + base64(png);
}

} // namespace

std::optional<uint32_t> parseColor(const std::string &text)
{
    if (text.size() != 7 && text.size() != 9) return std::nullopt;
    if (text[0] != '#') return std::nullopt;
    uint32_t value = 0;
    for (size_t index = 1; index < text.size(); index++) {
        char character = text[index];
        if (!std::isxdigit((unsigned char)character)) return std::nullopt;
        value = (value << 4) | (uint32_t)(std::isdigit((unsigned char)character) ? character - '0' : std::tolower(character) - 'a' + 10);
    }
    return text.size() == 7 ? 0xFF000000u | value : value;
}

bool isColorText(const std::string &text)
{
    return text.empty() || parseColor(text).has_value();
}

std::string colorText(uint32_t argb)
{
    char buffer[10];
    if ((argb >> 24) == 0xFF)
        snprintf(buffer, sizeof(buffer), "#%06X", argb & 0xFFFFFF);
    else
        snprintf(buffer, sizeof(buffer), "#%08X", argb);
    return buffer;
}

std::string swatchURI(uint32_t argb)
{
    static std::mutex mutex;
    static std::map<uint32_t, std::string> cache;
    std::lock_guard<std::mutex> lock(mutex);
    auto found = cache.find(argb);
    if (found != cache.end()) return found->second;
    if (cache.size() > 64) cache.clear();
    return cache[argb] = encodeSwatch(argb);
}

} // namespace astral::settings
