#include "AstralReactors.h"

#include <chrono>
#include <condition_variable>
#include <deque>
#include <map>
#include <mutex>
#include <set>
#include <thread>

#include "AstralSession.h"

namespace {

using Clock = std::chrono::steady_clock;

constexpr size_t kFaces = 5;
constexpr size_t kMaxEntries = 512;
constexpr auto kRetryAfter = std::chrono::seconds(3);
constexpr auto kMaxRateLimitWait = std::chrono::seconds(10);

struct Entry {
    uint64_t count = 0;
    std::optional<nlohmann::json> users;
    std::optional<Clock::time_point> failed;
};

struct Job {
    std::string key;
    std::string url;
    uint64_t count;
};

std::mutex gMutex;
std::condition_variable gWake;
std::map<std::string, Entry> gEntries;
std::set<std::string> gInFlight;
std::deque<Job> gJobs;
astral::reactors::Fetcher gFetcher;
std::once_flag gWorkerStarted;

std::string encoded(const std::string &value)
{
    static const char *hex = "0123456789ABCDEF";
    std::string out;
    for (unsigned char character : value) {
        if ((character >= 'A' && character <= 'Z') || (character >= 'a' && character <= 'z') || (character >= '0' && character <= '9')) {
            out += (char)character;
        } else {
            out += '%';
            out += hex[character >> 4];
            out += hex[character & 15];
        }
    }
    return out;
}

std::optional<nlohmann::json> users(const std::string &body)
{
    auto parsed = nlohmann::json::parse(body, nullptr, false);
    if (!parsed.is_array()) return std::nullopt;
    nlohmann::json list = nlohmann::json::array();
    for (const auto &user : parsed) {
        if (!user.is_object() || !user.contains("id") || !user["id"].is_string()) continue;
        nlohmann::json entry = {{"id", user["id"]}};
        for (const char *field : {"username", "global_name", "avatar"}) {
            if (user.contains(field) && user[field].is_string()) entry[field] = user[field];
        }
        list.push_back(entry);
        if (list.size() >= kFaces) break;
    }
    if (list.empty()) return std::nullopt;
    return list;
}

void store(const std::string &key, uint64_t count, std::optional<nlohmann::json> found)
{
    std::lock_guard<std::mutex> lock(gMutex);
    gInFlight.erase(key);
    if (gEntries.size() >= kMaxEntries) gEntries.clear();
    Entry entry;
    entry.count = count;
    entry.users = std::move(found);
    if (!entry.users) entry.failed = Clock::now();
    gEntries[key] = std::move(entry);
}

std::chrono::milliseconds rateLimitWait(const std::string &body)
{
    auto parsed = nlohmann::json::parse(body, nullptr, false);
    if (!parsed.is_object() || !parsed.contains("retry_after") || !parsed["retry_after"].is_number()) return std::chrono::milliseconds(0);
    auto wait = std::chrono::milliseconds((long long)(parsed["retry_after"].get<double>() * 1000));
    return std::min<std::chrono::milliseconds>(wait, kMaxRateLimitWait);
}

void work()
{
    while (true) {
        Job job;
        astral::reactors::Fetcher fetcher;
        {
            std::unique_lock<std::mutex> lock(gMutex);
            gWake.wait(lock, [] { return !gJobs.empty(); });
            job = gJobs.front();
            gJobs.pop_front();
            fetcher = gFetcher;
        }
        std::string token = astral::session::token();
        if (!fetcher || token.empty()) {
            store(job.key, job.count, std::nullopt);
            continue;
        }
        astral::reactors::FetchResult result = fetcher(job.url, token);
        store(job.key, job.count, result.status == 200 ? users(result.body) : std::nullopt);
        if (result.status == 429) std::this_thread::sleep_for(rateLimitWait(result.body));
    }
}

} // namespace

namespace astral::reactors {

void setFetcher(Fetcher fetcher)
{
    std::lock_guard<std::mutex> lock(gMutex);
    gFetcher = std::move(fetcher);
}

std::optional<nlohmann::json> lookup(const std::string &channelID, const std::string &messageID, const std::string &emoji, bool burst,
                                     uint64_t count, bool &fresh)
{
    fresh = false;
    if (channelID.empty() || messageID.empty() || emoji.empty() || !count) return std::nullopt;
    std::call_once(gWorkerStarted, [] { std::thread(work).detach(); });
    std::string key = channelID + ":" + messageID + ":" + emoji + ":" + (burst ? "1" : "0");
    std::lock_guard<std::mutex> lock(gMutex);
    auto found = gEntries.find(key);
    const Entry *entry = found == gEntries.end() ? nullptr : &found->second;
    bool stale = !entry || entry->count != count;
    bool coolingDown = entry && entry->failed && Clock::now() - *entry->failed < kRetryAfter && !stale;
    bool fetch = (stale || (entry && entry->failed && !coolingDown)) && !gInFlight.count(key);
    if (fetch) {
        gInFlight.insert(key);
        gJobs.push_back({key,
                         "https://discord.com/api/v9/channels/" + channelID + "/messages/" + messageID + "/reactions/" + encoded(emoji) +
                             "?limit=" + std::to_string(kFaces) + "&type=" + (burst ? "1" : "0"),
                         count});
        gWake.notify_one();
    }
    fresh = !fetch && entry && entry->users && entry->count == count;
    return entry ? entry->users : std::nullopt;
}

} // namespace astral::reactors
