#pragma once
#include <stddef.h>
#include <stdint.h>

typedef void (*AstralWireSink)(void *context, const char *line, size_t length);

int astral_wire_summarise(const uint8_t *bytes, size_t length, AstralWireSink sink, void *context);
