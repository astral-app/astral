#pragma once

#include <functional>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>

#include "AstralRust.h"

extern "C" {
typedef int (*AstralMetricKitImageCheck)(const char *binary, size_t length);
void astral_metrickit_set_image_check(AstralMetricKitImageCheck check);
AstralRustString astral_metrickit_stack(int kind, const char *json, size_t length);
}

namespace astral {
using MetricKitImageCheck = std::function<bool(const std::string &binaryName)>;

inline MetricKitImageCheck &metricKitImageCheck()
{
    static MetricKitImageCheck check;
    return check;
}

inline void setMetricKitImageCheck(MetricKitImageCheck check)
{
    metricKitImageCheck() = std::move(check);
    astral_metrickit_set_image_check([](const char *binary, size_t length) {
        auto &current = metricKitImageCheck();
        return current && current(std::string(binary, length)) ? 1 : 0;
    });
}

inline std::optional<std::string> metricKitStack(int kind, const nlohmann::json &diagnostic)
{
    std::string text = diagnostic.dump();
    return rust::takeIfPresent(astral_metrickit_stack(kind, text.data(), text.size()));
}

inline std::optional<std::string> metricKitCrashStack(const nlohmann::json &crash)
{
    return metricKitStack(0, crash);
}

inline std::optional<std::string> metricKitHangStack(const nlohmann::json &hang)
{
    return metricKitStack(1, hang);
}

inline std::optional<std::string> metricKitDiskWriteStack(const nlohmann::json &diskWrite)
{
    return metricKitStack(2, diskWrite);
}
} // namespace astral
