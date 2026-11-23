#include "AstralSession.h"

#include <mutex>

namespace {

std::mutex gMutex;
std::string gToken;

bool tokenCharacter(char character)
{
    return (character >= 'A' && character <= 'Z') || (character >= 'a' && character <= 'z') || (character >= '0' && character <= '9') ||
           character == '_' || character == '-';
}

} // namespace

namespace astral::session {

bool plausibleToken(const std::string &token)
{
    size_t first = token.find('.');
    size_t second = first == std::string::npos ? std::string::npos : token.find('.', first + 1);
    if (second == std::string::npos || token.find('.', second + 1) != std::string::npos) return false;
    if (first < 20 || second - first - 1 < 4 || token.size() - second - 1 < 20) return false;
    for (char character : token)
        if (character != '.' && !tokenCharacter(character)) return false;
    return true;
}

void setToken(const std::string &token)
{
    if (!plausibleToken(token)) return;
    std::lock_guard<std::mutex> lock(gMutex);
    gToken = token;
}

std::string token()
{
    std::lock_guard<std::mutex> lock(gMutex);
    return gToken;
}

} // namespace astral::session
