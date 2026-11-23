#include "AstralSettingsNavigator.h"

#include <cmath>
#include <mutex>

#include "AstralSettingsPages.h"
#include "PhoenixWire.h"

namespace {

const std::string kOverviewRow = "settings.row.astral";
const std::string kOverviewRowPrefix = "settings.row.astral.";
const std::string kBack = "astral.back";
const std::string kRowPrefix = "astral.row.";
const std::string kSwitchPrefix = "astral.switch.";
const std::string kSliderPrefix = "astral.slider.";

enum class Delivery { Push, Refresh, Pop };

std::recursive_mutex gMutex;
std::vector<std::string> gRoutes;
astral::navigator::Deliver gDeliver;
std::function<void(const std::function<void()> &)> gWithWire;

bool startsWith(const std::string &value, const std::string &prefix)
{
    return value.rfind(prefix, 0) == 0;
}

void append(std::vector<uint8_t> &merged, AstralWireBuffer &buffer)
{
    if (buffer.bytes && buffer.length > 1) {
        if (merged.empty())
            merged.assign(buffer.bytes, buffer.bytes + buffer.length);
        else
            merged.insert(merged.end(), buffer.bytes + 1, buffer.bytes + buffer.length);
    }
    astral_wire_buffer_free(&buffer);
}

void build(Delivery delivery, const std::string &route, std::vector<uint8_t> &merged, bool &built)
{
    AstralWireBuffer buffer = {};
    int made = 0;
    if (delivery == Delivery::Pop) {
        made = astral_page_pop(&buffer);
    } else {
        astral::pages::withSpec(astral::pages::model(route), [&](const AstralPageSpec *page) {
            made = delivery == Delivery::Push ? astral_page_push(page, &buffer) : astral_page_refresh(page, &buffer);
        });
        if (made && !astral_wire_unsupported_version()) astral_wire_theme_page(&buffer);
    }
    if (!made) {
        astral_wire_buffer_free(&buffer);
        return;
    }
    built = true;
    append(merged, buffer);
}

void deliver(std::vector<uint8_t> bytes)
{
    if (bytes.empty() || !gDeliver) return;
    astral_wire_note_identifiers(bytes.data(), bytes.size());
    gDeliver(std::move(bytes));
}

void run(const std::vector<std::pair<Delivery, std::string>> &steps)
{
    std::vector<uint8_t> merged;
    bool built = false;
    auto work = [&] {
        for (const auto &[delivery, route] : steps) build(delivery, route, merged, built);
    };
    if (gWithWire)
        gWithWire(work);
    else
        work();
    deliver(std::move(merged));
}

void push(const std::string &route)
{
    gRoutes.push_back(route);
    size_t depth = gRoutes.size();
    std::vector<uint8_t> merged;
    bool built = false;
    auto work = [&] { build(Delivery::Push, route, merged, built); };
    if (gWithWire)
        gWithWire(work);
    else
        work();
    if (!built && gRoutes.size() == depth) gRoutes.pop_back();
    deliver(std::move(merged));
}

void refreshTop()
{
    if (gRoutes.empty()) return;
    run({{Delivery::Refresh, gRoutes.back()}});
}

void pop()
{
    if (gRoutes.empty()) return;
    gRoutes.pop_back();
    if (gRoutes.empty())
        run({{Delivery::Pop, ""}});
    else
        run({{Delivery::Pop, ""}, {Delivery::Refresh, gRoutes.back()}});
}

void act(const std::string &key)
{
    if (gRoutes.empty()) return;
    auto next = astral::pages::action(gRoutes.back(), key, [] { astral::navigator::refresh(); });
    if (next && *next == "..")
        pop();
    else if (next && !next->empty())
        push(*next);
    else
        refreshTop();
}

} // namespace

namespace astral::navigator {

void configure(Deliver deliver, std::function<void(const std::function<void()> &)> withWire)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    gDeliver = std::move(deliver);
    gWithWire = std::move(withWire);
}

bool handles(const std::string &identifier)
{
    return identifier == kOverviewRow || startsWith(identifier, kOverviewRowPrefix) || identifier == kBack ||
           startsWith(identifier, kRowPrefix) || startsWith(identifier, kSwitchPrefix) || startsWith(identifier, kSliderPrefix);
}

void tap(const std::string &identifier)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (identifier == kOverviewRow || startsWith(identifier, kOverviewRowPrefix)) {
        if (astral_page_depth() > 0) return;
        gRoutes.clear();
        push(startsWith(identifier, kOverviewRowPrefix) ? identifier.substr(kOverviewRowPrefix.size()) : std::string());
    } else if (identifier == kBack) {
        pop();
    } else if (startsWith(identifier, kSwitchPrefix)) {
        act("switch." + identifier.substr(kSwitchPrefix.size()));
    } else if (startsWith(identifier, kRowPrefix)) {
        act(identifier.substr(kRowPrefix.size()));
    }
}

void slide(const std::string &identifier, double fraction)
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (!startsWith(identifier, kSliderPrefix) || gRoutes.empty()) return;
    std::string rest = identifier.substr(kSliderPrefix.size());
    size_t dot = rest.find('.');
    if (dot == std::string::npos) return;
    int steps = atoi(rest.substr(0, dot).c_str());
    if (steps <= 0) return;
    long position = std::lround(std::fmin(1, std::fmax(0, fraction)) * steps);
    astral::pages::action(gRoutes.back(), "number." + rest.substr(dot + 1) + "=" + std::to_string(position), [] {});
    refreshTop();
}

void refresh()
{
    std::lock_guard<std::recursive_mutex> lock(gMutex);
    if (astral_page_depth() > 0) refreshTop();
}

bool open()
{
    return astral_page_depth() > 0;
}

} // namespace astral::navigator
