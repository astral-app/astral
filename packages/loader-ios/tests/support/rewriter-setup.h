#pragma once
#include <stdio.h>
#include <string.h>

static inline size_t setupDecorator(const AstralDecorationContext *context, AstralTag *tags, size_t capacity)
{
    (void)context;
    if (!capacity) return 0;
    memset(&tags[0], 0, sizeof tags[0]);
    strcpy(tags[0].text, "MOD");
    tags[0].background = 0xFF4E7FFF;
    tags[0].foreground = 0xFFFFFFFF;
    tags[0].shape = kAstralTagPill;
    return 1;
}

static inline int setupRelabeler(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity)
{
    (void)context;
    int written = snprintf(out, capacity, "%.*s (@user)", (int)length, text);
    return written > 0 && (size_t)written < capacity ? written : 0;
}

static inline void setupRewriter(void)
{
    static const AstralRowSpec rows[] = {
        {"general", "astral", NULL, NULL, "res://astral_galaxy", kAstralRowLink, 0},
        {"plugins", "Plugins", NULL, NULL, NULL, kAstralRowLink, 0},
    };
    static const uint32_t from[] = {0xFF000000, 0xFFFFFFFF};
    static const uint32_t to[] = {0xFF101020, 0xFFEEEEFF};
    astral_wire_set_category("astral", rows, 2);
    astral_wire_set_decorator(setupDecorator);
    astral_wire_set_relabeler(setupRelabeler);
    astral_wire_set_palette(from, to, 2);
    astral_wire_set_scales(1.1, 1.05);
}
