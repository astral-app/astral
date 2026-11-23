#include "PhoenixWireInternal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { kMaxIdentifiers = 2048, kMaxIdentifier = 112 };

typedef struct {
    int32_t id;
    char identifier[kMaxIdentifier];
} NamedNode;

static NamedNode gNamed[kMaxIdentifiers];

static int routable(const uint8_t *text, uint32_t length)
{
    static const char *const prefixes[] = {"astral.", "settings.row.astral"};
    for (size_t index = 0; index < sizeof(prefixes) / sizeof(prefixes[0]); index++) {
        size_t prefix = strlen(prefixes[index]);
        if (length >= prefix && memcmp(text, prefixes[index], prefix) == 0) return 1;
    }
    return 0;
}

static NamedNode *slotFor(int32_t id, int create)
{
    uint32_t start = ((uint32_t)id * 2654435761u) % kMaxIdentifiers;
    for (uint32_t probe = 0; probe < kMaxIdentifiers; probe++) {
        NamedNode *slot = &gNamed[(start + probe) % kMaxIdentifiers];
        if (slot->id == id) return slot;
        if (slot->id == 0) return create ? slot : NULL;
    }
    return create ? &gNamed[start] : NULL;
}

static void noteSpan(const uint8_t *bytes, const Span *span)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property != P(accessibilityId) || tag != 6) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        uint32_t length = wireRead32(&cursor);
        if (!wireNeed(&cursor, length)) return;
        const uint8_t *text = bytes + cursor.offset;
        NamedNode *existing = slotFor(span->id, 0);
        if (!routable(text, length) || length >= kMaxIdentifier) {
            if (existing) existing->identifier[0] = 0;
            return;
        }
        NamedNode *slot = existing ? existing : slotFor(span->id, 1);
        slot->id = span->id;
        memcpy(slot->identifier, text, length);
        slot->identifier[length] = 0;
        return;
    }
}

void astral_wire_note_identifiers(const uint8_t *bytes, size_t length)
{
    size_t count = 0;
    Span *spans = malloc(sizeof(Span) * kMaxSpans);
    if (!spans) return;
    if (length && bytes[0] == kWireVersion && wireScan(bytes, length, spans, &count) == 0) {
        for (size_t index = 0; index < count; index++) {
            if (spans[index].op == 1 || spans[index].op == 2) noteSpan(bytes, &spans[index]);
        }
    }
    free(spans);
}

int astral_wire_identifier(int32_t id, char *out, size_t capacity)
{
    NamedNode *slot = slotFor(id, 0);
    if (!slot || !slot->identifier[0] || !capacity) return 0;
    snprintf(out, capacity, "%s", slot->identifier);
    return 1;
}
