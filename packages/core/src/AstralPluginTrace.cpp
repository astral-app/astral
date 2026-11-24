#include "AstralPluginTrace.h"

#include <fcntl.h>
#include <sys/mman.h>
#include <unistd.h>

#include <cstring>
#include <mutex>

#include "AstralFiles.h"

namespace astral::trace {
namespace {

constexpr const char kFile[] = "plugin-trace";
constexpr const char kMagic[8] = {'a', 's', 't', 'r', 'l', 't', 'r', '1'};
constexpr size_t kMaxDepth = 16;

struct Record {
    char magic[8];
    int64_t started;
    char active[128];
    char last[128];
    char running[3840];
};

static_assert(sizeof(Record) == 4112);

std::mutex gLock;
Record *gRecord;
std::optional<PreviousSession> gPrevious;
std::string gStack[kMaxDepth];
size_t gDepth;

bool identifierCharacter(char character)
{
    return (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') || (character >= '0' && character <= '9') ||
           character == '.' || character == '-' || character == '_';
}

std::string field(const char *value, size_t capacity)
{
    std::string text;
    for (size_t index = 0; index < capacity && value[index]; index++) {
        if (!identifierCharacter(value[index])) return {};
        text.push_back(value[index]);
    }
    return text;
}

std::vector<std::string> list(const char *value, size_t capacity)
{
    std::vector<std::string> identifiers;
    std::string current;
    for (size_t index = 0; index <= capacity; index++) {
        char character = index < capacity ? value[index] : 0;
        if (character == ',' || character == 0) {
            if (!current.empty()) identifiers.push_back(current);
            current.clear();
            if (character == 0) break;
            continue;
        }
        if (!identifierCharacter(character)) return {};
        current.push_back(character);
    }
    return identifiers;
}

void put(char *destination, size_t capacity, const std::string &value)
{
    size_t length = value.size() < capacity - 1 ? value.size() : capacity - 1;
    std::memcpy(destination, value.data(), length);
    std::memset(destination + length, 0, capacity - length);
}

std::optional<PreviousSession> read(const Record &record)
{
    if (std::memcmp(record.magic, kMagic, sizeof(kMagic)) != 0) return std::nullopt;
    PreviousSession session{record.started, field(record.active, sizeof(record.active)), field(record.last, sizeof(record.last)),
                            list(record.running, sizeof(record.running))};
    if (session.active.empty() && session.last.empty() && session.running.empty()) return std::nullopt;
    return session;
}

} // namespace

std::optional<PreviousSession> open(const std::string &directory, int64_t now)
{
    std::lock_guard<std::mutex> lock(gLock);
    if (gRecord) return gPrevious;
    int descriptor = ::open(joinPath(directory, kFile).c_str(), O_RDWR | O_CREAT | O_CLOEXEC, 0600);
    if (descriptor < 0) return std::nullopt;
    if (ftruncate(descriptor, sizeof(Record)) != 0) {
        ::close(descriptor);
        return std::nullopt;
    }
    void *mapped = mmap(nullptr, sizeof(Record), PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0);
    ::close(descriptor);
    if (mapped == MAP_FAILED) return std::nullopt;
    gRecord = static_cast<Record *>(mapped);
    gPrevious = read(*gRecord);
    std::memset(gRecord, 0, sizeof(Record));
    std::memcpy(gRecord->magic, kMagic, sizeof(kMagic));
    gRecord->started = now;
    gDepth = 0;
    return gPrevious;
}

std::optional<PreviousSession> previous()
{
    std::lock_guard<std::mutex> lock(gLock);
    return gPrevious;
}

void enter(const std::string &plugin)
{
    std::lock_guard<std::mutex> lock(gLock);
    if (gDepth < kMaxDepth) gStack[gDepth] = plugin;
    gDepth++;
    if (!gRecord) return;
    put(gRecord->active, sizeof(gRecord->active), plugin);
    put(gRecord->last, sizeof(gRecord->last), plugin);
}

void leave()
{
    std::lock_guard<std::mutex> lock(gLock);
    if (gDepth == 0) return;
    gDepth--;
    if (!gRecord) return;
    std::string outer = gDepth > 0 && gDepth <= kMaxDepth ? gStack[gDepth - 1] : std::string();
    put(gRecord->active, sizeof(gRecord->active), outer);
}

void setRunning(const std::vector<std::string> &plugins)
{
    std::lock_guard<std::mutex> lock(gLock);
    if (!gRecord) return;
    std::string joined;
    for (const auto &plugin : plugins) {
        if (joined.size() + plugin.size() + 1 >= sizeof(gRecord->running)) break;
        if (!joined.empty()) joined.push_back(',');
        joined += plugin;
    }
    put(gRecord->running, sizeof(gRecord->running), joined);
}

void close()
{
    std::lock_guard<std::mutex> lock(gLock);
    if (gRecord) munmap(gRecord, sizeof(Record));
    gRecord = nullptr;
    gPrevious.reset();
    gDepth = 0;
}

} // namespace astral::trace
