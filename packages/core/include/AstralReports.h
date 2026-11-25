#pragma once

#include <nlohmann/json.hpp>
#include <string>

#include "AstralRust.h"

extern "C" {
void astral_reports_configure(const char *directory, size_t length);
AstralRustString astral_reports_list(void);
AstralRustString astral_reports_fingerprint(const char *key, size_t length);
int astral_reports_add(const char *report, size_t length);
int astral_reports_plugin_failure(const char *identifier, size_t identifierLength, const char *name, size_t nameLength, const char *error,
                                  size_t errorLength, const char *environment, size_t environmentLength);
int astral_reports_unexpected_exit(const char *identifier, size_t identifierLength, const char *environment, size_t environmentLength,
                                   const char *running, size_t runningLength);
int astral_reports_process_exit(const char *kind, size_t kindLength, int64_t time, int64_t reason, const char *detail, size_t detailLength,
                                const char *environment, size_t environmentLength, const char *active, size_t activeLength,
                                const char *running, size_t runningLength);
AstralRustString astral_reports_text(const char *report, size_t length);
AstralRustString astral_reports_issue_url(const char *report, size_t length);
}

namespace astral::reports {
inline std::string json(const nlohmann::json &value)
{
    return value.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
}

inline std::string fingerprint(const std::string &key)
{
    return rust::take(astral_reports_fingerprint(key.data(), key.size()));
}

inline void configure(const std::string &directory)
{
    astral_reports_configure(directory.data(), directory.size());
}

inline nlohmann::json list()
{
    nlohmann::json reports = nlohmann::json::parse(rust::take(astral_reports_list()), nullptr, false);
    return reports.is_array() ? reports : nlohmann::json::array();
}

inline bool add(const nlohmann::json &report)
{
    std::string text = json(report);
    return astral_reports_add(text.data(), text.size());
}

inline bool pluginFailure(const std::string &identifier, const std::string &name, const std::string &error,
                          const nlohmann::json &environment)
{
    std::string text = json(environment);
    return astral_reports_plugin_failure(identifier.data(), identifier.size(), name.data(), name.size(), error.data(), error.size(),
                                         text.data(), text.size());
}

inline bool unexpectedExit(const std::string &identifier, const nlohmann::json &environment, const nlohmann::json &running)
{
    std::string environmentText = json(environment), runningText = json(running);
    return astral_reports_unexpected_exit(identifier.data(), identifier.size(), environmentText.data(), environmentText.size(),
                                          runningText.data(), runningText.size());
}

inline bool processExit(const std::string &kind, int64_t time, int64_t reason, const std::string &detail, const nlohmann::json &environment,
                        const std::string &active, const nlohmann::json &running)
{
    std::string environmentText = json(environment), runningText = json(running);
    return astral_reports_process_exit(kind.data(), kind.size(), time, reason, detail.data(), detail.size(), environmentText.data(),
                                       environmentText.size(), active.data(), active.size(), runningText.data(), runningText.size());
}

inline std::string text(const nlohmann::json &report)
{
    std::string value = json(report);
    return rust::take(astral_reports_text(value.data(), value.size()));
}

inline std::string issueURL(const nlohmann::json &report)
{
    std::string value = json(report);
    return rust::take(astral_reports_issue_url(value.data(), value.size()));
}
} // namespace astral::reports
