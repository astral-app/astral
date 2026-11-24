#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace astral::trace {

struct PreviousSession {
    int64_t started = 0;
    std::string active;
    std::string last;
    std::vector<std::string> running;
};

std::optional<PreviousSession> open(const std::string &directory, int64_t now);
std::optional<PreviousSession> previous();
void enter(const std::string &plugin);
void leave();
void setRunning(const std::vector<std::string> &plugins);
void close();

} // namespace astral::trace
