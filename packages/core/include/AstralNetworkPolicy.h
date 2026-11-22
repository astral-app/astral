#pragma once

#include <optional>
#include <string>
#include <string_view>

namespace astral {
bool isDiscordHost(std::string_view host);
bool isTelemetryRequest(std::string_view host, std::string_view path);
bool isCurrentUserRequest(std::string_view host, std::string_view path);
std::optional<std::string> withStaffUser(std::string_view body);
std::optional<std::string> withStaffReady(std::string_view message);
} // namespace astral
