#pragma once

#include <array>
#include <functional>
#include <string>
#include <vector>

#include "AstralLogCategories.h"
#include "AstralRust.h"

extern "C" {
typedef void (*AstralLogCallback)(void *context, int level, const char *category, size_t categoryLength, const char *message,
                                  size_t messageLength);
typedef void (*AstralSystemLogCallback)(void *context, int level, const char *line, size_t length);
void astral_set_log_sink(AstralLogCallback callback, void *context, void (*release)(void *));
void astral_set_system_log_sink(AstralSystemLogCallback callback, void *context, void (*release)(void *));
void astral_write_system_log(int level, const char *line, size_t length);
void astral_log(int level, const char *category, size_t categoryLength, const char *message, size_t messageLength);
int astral_is_log_category(const char *category, size_t length);
int astral_log_level_from_index(int index);
AstralRustString astral_format_log_line(const char *category, size_t categoryLength, const char *message, size_t messageLength);
void astral_log_from_platform(int level, const char *category, size_t categoryLength, const char *message, size_t messageLength);
}

namespace astral {
enum class LogLevel {
    Debug,
    Info,
    Warning,
    Error,
};

inline constexpr std::array<const char *, 12> kLogCategories = {
    LOG_CATEGORY_DEFAULT, LOG_CATEGORY_RUNTIME,    LOG_CATEGORY_UPDATER,   LOG_CATEGORY_THEMES,
    LOG_CATEGORY_FONTS,   LOG_CATEGORY_PLUGINS,    LOG_CATEGORY_SETTINGS,  LOG_CATEGORY_TOOLBOX,
    LOG_CATEGORY_HOOKS,   LOG_CATEGORY_FILESYSTEM, LOG_CATEGORY_UTILITIES, LOG_CATEGORY_DEBUGGER,
};

using LogSink = std::function<void(LogLevel, const std::string &, const std::string &)>;
using SystemLogSink = std::function<void(LogLevel, const std::string &)>;

inline void setLogSink(LogSink sink)
{
    if (!sink) {
        astral_set_log_sink(nullptr, nullptr, nullptr);
        return;
    }
    astral_set_log_sink(
        [](void *context, int level, const char *category, size_t categoryLength, const char *message, size_t messageLength) {
            (*static_cast<LogSink *>(context))(static_cast<LogLevel>(level), std::string(category, categoryLength),
                                               std::string(message, messageLength));
        },
        new LogSink(std::move(sink)), [](void *context) { delete static_cast<LogSink *>(context); });
}

inline void setSystemLogSink(SystemLogSink sink)
{
    if (!sink) {
        astral_set_system_log_sink(nullptr, nullptr, nullptr);
        return;
    }
    astral_set_system_log_sink(
        [](void *context, int level, const char *line, size_t length) {
            (*static_cast<SystemLogSink *>(context))(static_cast<LogLevel>(level), std::string(line, length));
        },
        new SystemLogSink(std::move(sink)), [](void *context) { delete static_cast<SystemLogSink *>(context); });
}

inline void writeSystemLog(LogLevel level, const std::string &line)
{
    astral_write_system_log(static_cast<int>(level), line.data(), line.size());
}

inline void log(LogLevel level, const std::string &category, const std::string &message)
{
    astral_log(static_cast<int>(level), category.data(), category.size(), message.data(), message.size());
}

inline bool isLogCategory(const std::string &category)
{
    return astral_is_log_category(category.data(), category.size());
}

inline LogLevel logLevelFromIndex(int index)
{
    return static_cast<LogLevel>(astral_log_level_from_index(index));
}

inline std::string formatLogLine(const std::string &category, const std::string &message)
{
    return rust::take(astral_format_log_line(category.data(), category.size(), message.data(), message.size()));
}

inline std::string scopedLogLine(const std::vector<std::string> &scope, const std::string &message)
{
    std::string joined;
    for (const std::string &part : scope) joined += (joined.empty() ? "" : "/") + part;
    return joined.empty() ? message : formatLogLine(joined, message);
}

inline void logFromPlatform(int level, const std::string &category, const std::string &message)
{
    astral_log_from_platform(level, category.data(), category.size(), message.data(), message.size());
}
} // namespace astral
