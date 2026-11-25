#pragma once

#include <functional>
#include <optional>
#include <string>
#include <string_view>

#include "AstralRust.h"

extern "C" {
typedef int (*AstralOutgoingTransform)(void *context, const char *channel, size_t channelLength, const char *operation,
                                       size_t operationLength, const char *content, size_t contentLength, AstralRustString *replacement);
int astral_is_discord_host(const char *host, size_t length);
int astral_is_telemetry_request(const char *host, size_t hostLength, const char *path, size_t pathLength);
int astral_is_current_user_request(const char *host, size_t hostLength, const char *path, size_t pathLength);
int astral_outgoing_message_target(const char *host, size_t hostLength, const char *path, size_t pathLength, const char *method,
                                   size_t methodLength, AstralRustString *channel, AstralRustString *operation);
AstralRustString astral_transform_outgoing_message_body(const char *host, size_t hostLength, const char *path, size_t pathLength,
                                                        const char *method, size_t methodLength, const char *body, size_t bodyLength,
                                                        AstralOutgoingTransform transform, void *context);
AstralRustString astral_with_staff_user(const char *body, size_t length);
AstralRustString astral_with_staff_ready(const char *message, size_t length);
}

namespace astral {
struct OutgoingMessageTarget {
    std::string channel;
    std::string operation;
};

using OutgoingMessageTransform = std::function<std::optional<std::string>(const OutgoingMessageTarget &, std::string_view content)>;

inline bool isDiscordHost(std::string_view host)
{
    return astral_is_discord_host(host.data(), host.size());
}

inline bool isTelemetryRequest(std::string_view host, std::string_view path)
{
    return astral_is_telemetry_request(host.data(), host.size(), path.data(), path.size());
}

inline bool isCurrentUserRequest(std::string_view host, std::string_view path)
{
    return astral_is_current_user_request(host.data(), host.size(), path.data(), path.size());
}

inline std::optional<OutgoingMessageTarget> outgoingMessageTarget(std::string_view host, std::string_view path, std::string_view method)
{
    AstralRustString channel{}, operation{};
    if (!astral_outgoing_message_target(host.data(), host.size(), path.data(), path.size(), method.data(), method.size(), &channel,
                                        &operation))
        return std::nullopt;
    return OutgoingMessageTarget{rust::take(channel), rust::take(operation)};
}

inline std::optional<std::string> transformOutgoingMessageBody(std::string_view host, std::string_view path, std::string_view method,
                                                               std::string_view body, const OutgoingMessageTransform &transform)
{
    if (!transform) return std::nullopt;
    auto call = [](void *context, const char *channel, size_t channelLength, const char *operation, size_t operationLength,
                   const char *content, size_t contentLength, AstralRustString *replacement) {
        const auto &transform = *static_cast<const OutgoingMessageTransform *>(context);
        auto result = transform(OutgoingMessageTarget{std::string(channel, channelLength), std::string(operation, operationLength)},
                                std::string_view(content, contentLength));
        if (!result) return 0;
        *replacement = astral_rust_string_from(result->data(), result->size());
        return 1;
    };
    return rust::takeIfPresent(astral_transform_outgoing_message_body(host.data(), host.size(), path.data(), path.size(), method.data(),
                                                                      method.size(), body.data(), body.size(), call,
                                                                      const_cast<OutgoingMessageTransform *>(&transform)));
}

inline std::optional<std::string> withStaffUser(std::string_view body)
{
    return rust::takeIfPresent(astral_with_staff_user(body.data(), body.size()));
}

inline std::optional<std::string> withStaffReady(std::string_view message)
{
    return rust::takeIfPresent(astral_with_staff_ready(message.data(), message.size()));
}
} // namespace astral
