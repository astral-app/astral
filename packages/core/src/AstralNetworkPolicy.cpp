#include "AstralNetworkPolicy.h"

#include <array>
#include <cctype>
#include <nlohmann/json.hpp>

namespace astral {
namespace {
constexpr long long kStaffFlag = 1;

constexpr std::array<std::string_view, 2> kDiscordDomains = {"discord.com", "discordapp.com"};

constexpr std::array<std::string_view, 8> kTrackerDomains = {
    "sentry.io",     "appsflyer.com",       "appsflyersdk.com",     "litix.io",
    "qualtrics.com", "app-measurement.com", "google-analytics.com", "crashlytics.com"};

bool endsWith(std::string_view text, std::string_view suffix)
{
    return text.size() >= suffix.size() && text.substr(text.size() - suffix.size()) == suffix;
}

std::string lowercase(std::string_view value)
{
    std::string result(value);
    for (char &character : result) character = static_cast<char>(std::tolower(static_cast<unsigned char>(character)));
    return result;
}

bool isUnder(std::string_view host, std::string_view domain)
{
    return host == domain || (host.size() > domain.size() && endsWith(host, domain) && host[host.size() - domain.size() - 1] == '.');
}

bool isUnderAny(std::string_view host, const auto &domains)
{
    for (std::string_view domain : domains)
        if (isUnder(host, domain)) return true;
    return false;
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

bool isRouteOrChild(std::string_view path, std::string_view route)
{
    return path == route || (path.size() > route.size() && path.substr(0, route.size()) == route && path[route.size()] == '/');
}

bool telemetryRoute(std::string_view path)
{
    std::string normalizedPath = lowercase(withoutQuery(path));
    path = normalizedPath;
    if (isRouteOrChild(path, "/error-reporting-proxy")) return true;
    auto route = apiRoute(path);
    if (!route) return false;
    return isRouteOrChild(*route, "/science") || isRouteOrChild(*route, "/metrics");
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
    std::string normalized = lowercase(host);
    return isUnderAny(normalized, kDiscordDomains);
}

bool isTelemetryRequest(std::string_view host, std::string_view path)
{
    std::string normalizedHost = lowercase(host);
    if (isUnderAny(normalizedHost, kTrackerDomains)) return true;
    return isUnderAny(normalizedHost, kDiscordDomains) && telemetryRoute(path);
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
