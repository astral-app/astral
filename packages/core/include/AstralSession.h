#pragma once

#include <string>

#include "AstralRust.h"

extern "C" {
int astral_session_plausible_token(const char *token, size_t length);
void astral_session_set_token(const char *token, size_t length);
AstralRustString astral_session_token(void);
}

namespace astral::session {
inline bool plausibleToken(const std::string &token)
{
    return astral_session_plausible_token(token.data(), token.size());
}

inline void setToken(const std::string &token)
{
    astral_session_set_token(token.data(), token.size());
}

inline std::string token()
{
    return rust::take(astral_session_token());
}
} // namespace astral::session
