#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "AstralRust.h"

extern "C" {
typedef struct {
    int64_t started;
    AstralRustString active;
    AstralRustString last;
    AstralRustString running;
} AstralPreviousSession;

int astral_trace_open(const char *directory, size_t length, int64_t now, AstralPreviousSession *session);
int astral_trace_previous(AstralPreviousSession *session);
void astral_trace_enter(const char *plugin, size_t length);
void astral_trace_leave(void);
void astral_trace_set_running(const char *plugins, size_t length);
void astral_trace_close(void);
}

namespace astral::trace {
struct PreviousSession {
    int64_t started = 0;
    std::string active;
    std::string last;
    std::vector<std::string> running;
};

inline std::optional<PreviousSession> taken(int found, AstralPreviousSession &session)
{
    if (!found) return std::nullopt;
    return PreviousSession{session.started, rust::take(session.active), rust::take(session.last), rust::takeList(session.running)};
}

inline std::optional<PreviousSession> open(const std::string &directory, int64_t now)
{
    AstralPreviousSession session{};
    return taken(astral_trace_open(directory.data(), directory.size(), now, &session), session);
}

inline std::optional<PreviousSession> previous()
{
    AstralPreviousSession session{};
    return taken(astral_trace_previous(&session), session);
}

inline void enter(const std::string &plugin)
{
    astral_trace_enter(plugin.data(), plugin.size());
}

inline void leave()
{
    astral_trace_leave();
}

inline void setRunning(const std::vector<std::string> &plugins)
{
    std::string joined;
    for (const std::string &plugin : plugins) joined.append(plugin).push_back('\0');
    astral_trace_set_running(joined.data(), joined.size());
}

inline void close()
{
    astral_trace_close();
}
} // namespace astral::trace
