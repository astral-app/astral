#include "LuaLocale.h"

#include <strings.h>

namespace astral::lua {
namespace {
std::string languageOf(const std::string &locale)
{
    return locale.substr(0, locale.find('-'));
}

bool sameLocale(const std::string &left, const std::string &right)
{
    return strcasecmp(left.c_str(), right.c_str()) == 0;
}

std::vector<std::string> wantedLocales(const std::vector<std::string> &preferred)
{
    std::vector<std::string> wanted;
    for (const auto &language : preferred) {
        wanted.push_back(language);
        wanted.push_back(languageOf(language));
    }
    for (const char *fallback : {"en-US", "en-GB", "en"}) wanted.emplace_back(fallback);
    return wanted;
}
} // namespace

std::optional<std::string> bestLocale(const std::vector<std::string> &available, const std::vector<std::string> &preferred)
{
    for (const auto &candidate : wantedLocales(preferred)) {
        for (const auto &locale : available) {
            if (sameLocale(locale, candidate)) return locale;
        }
        for (const auto &locale : available) {
            if (sameLocale(languageOf(locale), candidate)) return locale;
        }
    }
    return std::nullopt;
}

std::string fillPlaceholders(std::string text, const std::vector<std::pair<std::string, std::string>> &values)
{
    for (const auto &[name, value] : values) {
        std::string token = "{" + name + "}";
        for (size_t at = text.find(token); at != std::string::npos; at = text.find(token, at + value.size()))
            text.replace(at, token.size(), value);
    }
    return text;
}

bool validPluginIdentifier(const std::string &identifier)
{
    if (identifier.size() < 3 || identifier.size() > 80 || identifier.find("..") != std::string::npos) return false;
    for (char character : identifier) {
        bool allowed =
            (character >= 'a' && character <= 'z') || (character >= '0' && character <= '9') || character == '.' || character == '-';
        if (!allowed) return false;
    }
    return true;
}
} // namespace astral::lua
