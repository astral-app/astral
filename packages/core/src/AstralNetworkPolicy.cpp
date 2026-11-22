#include "AstralNetworkPolicy.h"

#include <array>
#include <nlohmann/json.hpp>

namespace astral {
namespace {
constexpr long long kStaffFlag = 1;

constexpr std::array<std::string_view, 4> kDiscordHosts = {"discord.com", "discordapp.com", "ptb.discord.com", "canary.discord.com"};

constexpr std::array<std::string_view, 5> kTelemetryRoutes = {"/science", "/metrics", "/metrics/v2", "/track", "/client-reporting"};

bool endsWith(std::string_view text, std::string_view suffix)
{
    return text.size() >= suffix.size() && text.substr(text.size() - suffix.size()) == suffix;
}

std::string_view withoutQuery(std::string_view path)
{
    return path.substr(0, path.find('?'));
}

std::optional<std::string_view> apiRoute(std::string_view path)
{
    path = withoutQuery(path);
    if (path.substr(0, 4) != "/api") return std::nullopt;
    path.remove_prefix(4);
    if (path.size() > 2 && path[0] == '/' && path[1] == 'v') {
        size_t digits = 2;
        while (digits < path.size() && path[digits] >= '0' && path[digits] <= '9') digits++;
        if (digits > 2) path.remove_prefix(digits);
    }
    while (path.size() > 1 && path.back() == '/') path.remove_suffix(1);
    return path;
}

bool markStaff(nlohmann::json &user)
{
    if (!user.is_object()) return false;
    for (const char *field : {"flags", "public_flags"}) {
        long long flags = user.contains(field) && user[field].is_number_integer() ? user[field].get<long long>() : 0;
        user[field] = flags | kStaffFlag;
    }
    return true;
}
} // namespace

bool isDiscordHost(std::string_view host)
{
    for (std::string_view known : kDiscordHosts) {
        if (host == known) return true;
    }
    return false;
}

bool isTelemetryRequest(std::string_view host, std::string_view path)
{
    if (host == "sentry.io" || endsWith(host, ".sentry.io")) return true;
    if (!isDiscordHost(host)) return false;
    auto route = apiRoute(path);
    if (!route) return false;
    for (std::string_view telemetry : kTelemetryRoutes) {
        if (*route == telemetry) return true;
    }
    return false;
}

bool isCurrentUserRequest(std::string_view host, std::string_view path)
{
    auto route = apiRoute(path);
    return isDiscordHost(host) && route && *route == "/users/@me";
}

std::optional<std::string> withStaffUser(std::string_view body)
{
    auto user = nlohmann::json::parse(body, nullptr, false);
    if (!markStaff(user)) return std::nullopt;
    return user.dump();
}

std::optional<std::string> withStaffReady(std::string_view message)
{
    if (message.find("\"READY\"") == std::string_view::npos) return std::nullopt;
    auto payload = nlohmann::json::parse(message, nullptr, false);
    if (!payload.is_object() || payload.value("t", nlohmann::json()) != "READY") return std::nullopt;
    auto data = payload.find("d");
    if (data == payload.end() || !data->is_object() || !data->contains("user")) return std::nullopt;
    if (!markStaff((*data)["user"])) return std::nullopt;
    return payload.dump();
}
} // namespace astral
