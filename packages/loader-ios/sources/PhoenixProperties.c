#include "PhoenixProperties.h"

#include <string.h>

#define ASTRAL_WIRE_DEFAULT(name, fallback) .name = fallback,
static const AstralWireProperties kFallback = {ASTRAL_WIRE_PROPERTIES(ASTRAL_WIRE_DEFAULT)};
#undef ASTRAL_WIRE_DEFAULT

#define ASTRAL_WIRE_DEFAULT(name, fallback) .name = fallback,
AstralWireProperties gAstralWireProperties = {ASTRAL_WIRE_PROPERTIES(ASTRAL_WIRE_DEFAULT)};
#undef ASTRAL_WIRE_DEFAULT

static int indexOf(const char *const *names, size_t count, const char *wanted, uint16_t *index)
{
    for (size_t position = 0; position < count && position <= UINT16_MAX; position++) {
        if (names[position] && strcmp(names[position], wanted) == 0) {
            *index = (uint16_t)position;
            return 1;
        }
    }
    return 0;
}

void astral_wire_reset_properties(void)
{
    gAstralWireProperties = kFallback;
}

size_t astral_wire_resolve_properties(const char *const *names, size_t count, const char **missing)
{
    AstralWireProperties resolved = kFallback;
    size_t found = 0;
#define ASTRAL_WIRE_RESOLVE(name, fallback)                                                                                                \
    if (indexOf(names, count, #name, &resolved.name))                                                                                      \
        found++;                                                                                                                           \
    else if (missing && !*missing)                                                                                                         \
        *missing = #name;
    ASTRAL_WIRE_PROPERTIES(ASTRAL_WIRE_RESOLVE)
#undef ASTRAL_WIRE_RESOLVE
    if (missing && *missing) return found;
    gAstralWireProperties = resolved;
    return found;
}
