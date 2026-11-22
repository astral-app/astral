#include "PhoenixWireInternal.h"

#include <stdio.h>
#include <stdlib.h>

enum { kImageCapacity = 8192, kImageNameLength = 64 };

typedef struct {
    int32_t id;
    char name[kImageNameLength];
} TrackedImage;

static TrackedImage gImages[kImageCapacity];
static const char kIconPrefix[] = "res://ic_";

static TrackedImage *imageSlot(int32_t id, int create)
{
    uint32_t start = ((uint32_t)id * 2654435761u) % kImageCapacity;
    for (uint32_t probe = 0; probe < kImageCapacity; probe++) {
        TrackedImage *slot = &gImages[(start + probe) % kImageCapacity];
        if (slot->id == id) return slot;
        if (!slot->id) return create ? slot : NULL;
    }
    return NULL;
}

static void trackUri(int32_t id, const uint8_t *text, uint32_t length)
{
    size_t prefix = sizeof(kIconPrefix) - 1;
    if (length <= prefix || memcmp(text, kIconPrefix, prefix) != 0) return;
    uint32_t nameLength = 0;
    while (nameLength < length - 6 && text[6 + nameLength] != '#') nameLength++;
    if (nameLength >= kImageNameLength) return;
    TrackedImage *slot = imageSlot(id, 1);
    if (!slot) return;
    slot->id = id;
    memcpy(slot->name, text + 6, nameLength);
    slot->name[nameLength] = 0;
}

static void observeImage(const uint8_t *bytes, const Span *span)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == P(uri) && tag == 6) {
            uint32_t length = wireRead32(&cursor);
            if (!wireNeed(&cursor, length)) return;
            trackUri(span->id, bytes + cursor.offset, length);
            return;
        }
        wireSkipValue(&cursor, tag, 0);
    }
}

void wireTrackImages(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if ((span->op == 1 && wireTypeIs(bytes, span, "image")) || (span->op == 2 && imageSlot(span->id, 0))) observeImage(bytes, span);
        if (span->op != 4) continue;
        for (uint32_t position = 0; position < span->childCount; position++) {
            TrackedImage *slot = imageSlot(wireChildAt(bytes, span, position), 0);
            if (slot) slot->name[0] = 0;
        }
    }
}

int astral_wire_refresh_images(AstralWireBuffer *buffer, unsigned generation)
{
    memset(buffer, 0, sizeof(*buffer));
    Out out = {0};
    wirePut8(&out, kWireVersion);
    int updates = 0;
    char uri[kImageNameLength + 32];
    for (size_t index = 0; index < kImageCapacity; index++) {
        const TrackedImage *image = &gImages[index];
        if (!image->id || !image->name[0]) continue;
        snprintf(uri, sizeof(uri), "res://%s#%u", image->name, generation);
        wirePut8(&out, 2);
        wirePut32(&out, (uint32_t)image->id);
        wirePut16(&out, 1);
        wirePropString(&out, P(uri), uri);
        updates++;
    }
    if (!updates || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return updates;
}
