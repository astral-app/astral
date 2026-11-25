#include "AstralHostRequests.h"

#include <cstring>

#include "AstralNetworkPolicy.h"
#include "AstralRequestSink.h"

namespace astral::hostrequests {

namespace {

constexpr size_t kHeader = 16;
constexpr uint8_t kCallKind = 1;

struct Reader {
    const std::vector<uint8_t> &bytes;
    size_t end;
    size_t offset;
    bool failed = false;

    bool need(size_t count)
    {
        if (failed || end - offset < count) failed = true;
        return !failed;
    }
    uint8_t u8() { return need(1) ? bytes[offset++] : 0; }
    uint16_t u16()
    {
        uint16_t value = 0;
        if (need(2)) std::memcpy(&value, bytes.data() + offset, 2), offset += 2;
        return value;
    }
    uint32_t u32()
    {
        uint32_t value = 0;
        if (need(4)) std::memcpy(&value, bytes.data() + offset, 4), offset += 4;
        return value;
    }
    std::string string()
    {
        uint32_t size = u32();
        if (!need(size)) return {};
        std::string value(reinterpret_cast<const char *>(bytes.data() + offset), size);
        offset += size;
        return value;
    }
    void skip(uint8_t tag, int depth = 0)
    {
        if (depth > 8) {
            failed = true;
            return;
        }
        switch (tag) {
        case 0:
        case 1:
        case 2:
            return;
        case 3:
            if (need(4)) offset += 4;
            return;
        case 4:
        case 5:
            if (need(8)) offset += 8;
            return;
        case 6:
            string();
            return;
        case 7: {
            uint32_t count = u32();
            for (uint32_t index = 0; index < count && !failed; index++) skip(u8(), depth + 1);
            return;
        }
        case 8: {
            uint32_t count = u32();
            for (uint32_t index = 0; index < count && !failed; index++) {
                u16();
                skip(u8(), depth + 1);
            }
            return;
        }
        case 10: {
            uint32_t count = u32();
            for (uint32_t index = 0; index < count && !failed; index++) {
                string();
                skip(u8(), depth + 1);
            }
            return;
        }
        default:
            failed = true;
        }
    }
};

struct Writer {
    std::vector<uint8_t> bytes;
    void u8(uint8_t value) { bytes.push_back(value); }
    void u16(uint16_t value) { bytes.insert(bytes.end(), reinterpret_cast<uint8_t *>(&value), reinterpret_cast<uint8_t *>(&value) + 2); }
    void u32(uint32_t value) { bytes.insert(bytes.end(), reinterpret_cast<uint8_t *>(&value), reinterpret_cast<uint8_t *>(&value) + 4); }
    void string(const std::string &value)
    {
        u8(6);
        u32(static_cast<uint32_t>(value.size()));
        bytes.insert(bytes.end(), value.begin(), value.end());
    }
    void raw(const uint8_t *data, size_t size) { bytes.insert(bytes.end(), data, data + size); }
};

bool IsMethod(const std::string &value)
{
    return value == "GET" || value == "POST" || value == "PUT" || value == "PATCH" || value == "DELETE" || value == "HEAD";
}

struct Fields {
    size_t argumentsEnd = 0;
    int method = -1;
    int url = -1;
    int headers = -1;
    luahost::HttpRequest request;
};

std::optional<std::vector<std::pair<std::string, std::string>>> ReadHeaders(Reader &reader)
{
    std::vector<std::pair<std::string, std::string>> headers;
    uint32_t count = reader.u32();
    if (count > 256) return std::nullopt;
    for (uint32_t index = 0; index < count && !reader.failed; index++) {
        if (reader.u8() != 7 || reader.u32() != 2) return std::nullopt;
        if (reader.u8() != 6) return std::nullopt;
        std::string name = reader.string();
        if (reader.u8() != 6) return std::nullopt;
        std::string value = reader.string();
        for (char &character : name) character = static_cast<char>(tolower(static_cast<unsigned char>(character)));
        headers.emplace_back(std::move(name), std::move(value));
    }
    if (reader.failed) return std::nullopt;
    return headers;
}

std::string PathOf(const std::string &url)
{
    size_t scheme = url.find("://");
    size_t start = scheme == std::string::npos ? std::string::npos : url.find('/', scheme + 3);
    if (start == std::string::npos) return "/";
    size_t end = url.find_first_of("?#", start);
    return url.substr(start, end == std::string::npos ? std::string::npos : end - start);
}

std::string HostOf(const std::string &url)
{
    size_t scheme = url.find("://");
    if (scheme == std::string::npos) return {};
    size_t start = scheme + 3;
    size_t end = url.find_first_of("/?#:", start);
    std::string host = url.substr(start, end == std::string::npos ? std::string::npos : end - start);
    for (char &character : host) character = static_cast<char>(tolower(static_cast<unsigned char>(character)));
    return host;
}

std::optional<Fields> Read(const std::vector<uint8_t> &frame)
{
    if (frame.size() < kHeader || frame[0] != kCallKind) return std::nullopt;
    uint32_t argumentsLength;
    std::memcpy(&argumentsLength, frame.data() + 12, 4);
    if (argumentsLength == 0 || argumentsLength > frame.size() - kHeader) return std::nullopt;
    Fields fields;
    fields.argumentsEnd = kHeader + argumentsLength;
    Reader reader{frame, fields.argumentsEnd, kHeader};
    if (reader.u8() != 8) return std::nullopt;
    uint32_t count = reader.u32();
    if (count > 32) return std::nullopt;
    for (uint32_t index = 0; index < count && !reader.failed; index++) {
        reader.u16();
        uint8_t tag = reader.u8();
        if (tag == 6) {
            std::string value = reader.string();
            if (fields.method < 0 && IsMethod(value)) {
                fields.method = static_cast<int>(index);
                fields.request.method = value;
            } else if (fields.url < 0 && value.rfind("https://", 0) == 0) {
                fields.url = static_cast<int>(index);
                fields.request.url = value;
            }
        } else if (tag == 7 && fields.headers < 0) {
            Reader probe = reader;
            auto headers = ReadHeaders(probe);
            if (headers) {
                fields.headers = static_cast<int>(index);
                fields.request.headers = std::move(*headers);
                reader.offset = probe.offset;
            } else {
                reader.skip(7);
            }
        } else {
            reader.skip(tag);
        }
    }
    if (reader.failed || fields.method < 0 || fields.url < 0 || fields.headers < 0) return std::nullopt;
    fields.request.path = PathOf(fields.request.url);
    fields.request.body.assign(reinterpret_cast<const char *>(frame.data() + fields.argumentsEnd), frame.size() - fields.argumentsEnd);
    return fields;
}

} // namespace

std::optional<luahost::HttpRequest> parse(const std::vector<uint8_t> &frame)
{
    auto fields = Read(frame);
    if (!fields) return std::nullopt;
    return fields->request;
}

std::vector<uint8_t> rebuild(const std::vector<uint8_t> &frame, const luahost::HttpRequest &request)
{
    auto fields = Read(frame);
    if (!fields) return frame;
    Reader reader{frame, fields->argumentsEnd, kHeader + 1};
    uint32_t count = reader.u32();
    Writer arguments;
    arguments.u8(8);
    arguments.u32(count);
    for (uint32_t index = 0; index < count && !reader.failed; index++) {
        uint16_t key = reader.u16();
        size_t valueStart = reader.offset;
        uint8_t tag = reader.u8();
        reader.skip(tag);
        arguments.u16(key);
        if (static_cast<int>(index) == fields->method) {
            arguments.string(request.method);
        } else if (static_cast<int>(index) == fields->url) {
            arguments.string(request.url);
        } else if (static_cast<int>(index) == fields->headers) {
            arguments.u8(7);
            arguments.u32(static_cast<uint32_t>(request.headers.size()));
            for (const auto &[name, value] : request.headers) {
                arguments.u8(7);
                arguments.u32(2);
                arguments.string(name);
                arguments.string(value);
            }
        } else {
            arguments.raw(frame.data() + valueStart, reader.offset - valueStart);
        }
    }
    if (reader.failed) return frame;
    Writer out;
    out.raw(frame.data(), 12);
    out.u32(static_cast<uint32_t>(arguments.bytes.size()));
    out.raw(arguments.bytes.data(), arguments.bytes.size());
    out.raw(reinterpret_cast<const uint8_t *>(request.body.data()), request.body.size());
    return out.bytes;
}

bool wanted(const std::vector<uint8_t> &frame)
{
    auto fields = Read(frame);
    return fields && luahost::wantsHttp(fields->request.method, HostOf(fields->request.url), fields->request.path);
}

std::optional<std::vector<uint8_t>> intercept(const std::vector<uint8_t> &frame)
{
    auto fields = Read(frame);
    if (!fields) return std::nullopt;
    luahost::HttpRequest request = fields->request;
    auto decision = luahost::interceptHttpRequest(request);
    if (decision.response) {
        std::string sink = astral::sink::respondWith(decision.response->status, decision.response->headers, decision.response->body);
        if (sink.empty()) return std::nullopt;
        luahost::HttpRequest answered = fields->request;
        answered.method = "GET";
        answered.url = sink;
        answered.path = PathOf(sink);
        answered.body.clear();
        std::vector<std::pair<std::string, std::string>> headers;
        for (const auto &[name, value] : answered.headers)
            if (name != "authorization" && name != "cookie" && name != "content-type") headers.emplace_back(name, value);
        answered.headers = std::move(headers);
        return rebuild(frame, answered);
    }
    if (!decision.changed) return std::nullopt;
    return rebuild(frame, request);
}

} // namespace astral::hostrequests
