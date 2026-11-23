#include "AstralPrivacy.h"

#include <regex>

namespace astral {

std::string sanitizeDiagnosticText(std::string_view text)
{
    std::string sanitized(text.substr(0, 6000));
    static const std::regex token(R"([A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{20,})");
    static const std::regex mfa(R"(mfa\.[A-Za-z0-9_-]{20,})", std::regex::icase);
    static const std::regex secret(
        R"(((?:authorization|access_token|refresh_token|handoff_token|handoff_key|token|password|secret|session)\s*[:=]\s*)(?:bearer\s+)?[^\s,;]+)",
        std::regex::icase);
    static const std::regex privateContent(R"(((?:message|content|body|cookie|set-cookie)\s*[:=]\s*)[^\r\n]*)", std::regex::icase);
    static const std::regex privatePath(
        R"((?:file://)?(?:/Users/|/home/|/var/mobile/|/private/var/|/data/user/[0-9]+/|/storage/emulated/[0-9]+/)[^\s]+)");
    static const std::regex query(R"((https?://[^\s?#]+)\?[^\s#]*)", std::regex::icase);
    sanitized = std::regex_replace(sanitized, token, "[redacted]");
    sanitized = std::regex_replace(sanitized, mfa, "[redacted]");
    sanitized = std::regex_replace(sanitized, secret, "$1[redacted]");
    sanitized = std::regex_replace(sanitized, privateContent, "$1[redacted]");
    sanitized = std::regex_replace(sanitized, privatePath, "[private path]");
    sanitized = std::regex_replace(sanitized, query, "$1?[redacted]");
    if (sanitized.size() > 6000) sanitized.resize(6000);
    return sanitized;
}

}
