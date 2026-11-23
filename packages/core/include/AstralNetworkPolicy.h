#pragma once

#include <functional>
#include <optional>
#include <string>
#include <string_view>

namespace astral {
struct OutgoingMessageTarget {
    std::string channel;
    std::string operation;
};

using OutgoingMessageTransform = std::function<std::optional<std::string>(const OutgoingMessageTarget &, std::string_view content)>;

bool isDiscordHost(std::string_view host);
bool isTelemetryRequest(std::string_view host, std::string_view path);
bool isCurrentUserRequest(std::string_view host, std::string_view path);
std::optional<OutgoingMessageTarget> outgoingMessageTarget(std::string_view host, std::string_view path, std::string_view method);
std::optional<std::string> transformOutgoingMessageBody(std::string_view host, std::string_view path, std::string_view method,
                                                        std::string_view body, const OutgoingMessageTransform &transform);
std::optional<std::string> withStaffUser(std::string_view body);
std::optional<std::string> withStaffReady(std::string_view message);
} // namespace astral
