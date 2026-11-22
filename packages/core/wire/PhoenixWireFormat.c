#include "PhoenixWireInternal.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void skipTagged(Cursor *cursor, int depth)
{
    wireSkipValue(cursor, wireRead8(cursor), depth);
}

void wireSkipValue(Cursor *cursor, uint8_t tag, int depth)
{
    if (depth > 8) {
        cursor->failed = 1;
        return;
    }
    switch (tag) {
    case 0:
    case 1:
    case 2:
        break;
    case 3:
        wireSkip(cursor, 4);
        break;
    case 4:
    case 5:
        wireSkip(cursor, 8);
        break;
    case 6:
        wireSkip(cursor, wireRead32(cursor));
        break;
    case 7: {
        uint32_t count = wireRead32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) skipTagged(cursor, depth + 1);
        break;
    }
    case 8: {
        uint32_t count = wireRead32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            wireSkip(cursor, 2);
            skipTagged(cursor, depth + 1);
        }
        break;
    }
    case 9:
        wireSkip(cursor, 16);
        break;
    case 10: {
        uint32_t count = wireRead32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            wireSkip(cursor, wireRead32(cursor));
            skipTagged(cursor, depth + 1);
        }
        break;
    }
    case 11:
        wireSkip(cursor, (size_t)wireRead32(cursor) * 4);
        break;
    case 12: {
        wireSkip(cursor, 4);
        wireSkip(cursor, (size_t)wireRead16(cursor) * 6);
        break;
    }
    default:
        cursor->failed = 1;
        break;
    }
}

static void skipProperties(Cursor *cursor)
{
    uint16_t count = wireRead16(cursor);
    for (uint16_t index = 0; index < count && !cursor->failed; index++) {
        wireSkip(cursor, 2);
        wireSkipValue(cursor, wireRead8(cursor), 0);
    }
}

int wireScan(const uint8_t *bytes, size_t length, Span *spans, size_t *count)
{
    Cursor cursor = {bytes, length, 0, 0};
    if (wireRead8(&cursor) != 1) return -1;
    size_t used = 0;
    while (!cursor.failed && cursor.offset < length) {
        if (used == kMaxSpans) return -1;
        Span *span = &spans[used++];
        memset(span, 0, sizeof(*span));
        span->start = cursor.offset;
        span->op = wireRead8(&cursor);
        switch (span->op) {
        case 1:
            span->id = (int32_t)wireRead32(&cursor);
            wireSkip(&cursor, wireRead16(&cursor));
            skipProperties(&cursor);
            break;
        case 2:
            span->id = (int32_t)wireRead32(&cursor);
            skipProperties(&cursor);
            break;
        case 3:
            span->id = (int32_t)wireRead32(&cursor);
            span->childCount = wireRead32(&cursor);
            span->children = cursor.offset;
            wireSkip(&cursor, (size_t)span->childCount * 4);
            break;
        case 4:
        case 9:
            span->childCount = wireRead32(&cursor);
            span->children = cursor.offset;
            wireSkip(&cursor, (size_t)span->childCount * 4);
            break;
        case 5:
            wireSkip(&cursor, 4);
            break;
        case 6:
            if (wireRead8(&cursor)) wireSkip(&cursor, 4);
            break;
        case 7:
            wireSkip(&cursor, 4);
            skipTagged(&cursor, 0);
            break;
        case 8:
            break;
        case 10: {
            span->id = (int32_t)wireRead32(&cursor);
            uint32_t edits = wireRead32(&cursor);
            for (uint32_t index = 0; index < edits && !cursor.failed; index++) {
                wireSkip(&cursor, 8);
                wireSkip(&cursor, (size_t)wireRead32(&cursor) * 4);
            }
            break;
        }
        default:
            return -1;
        }
        span->end = cursor.offset;
    }
    *count = used;
    return cursor.failed ? -1 : 0;
}

int32_t wireChild(const uint8_t *bytes, const Span *span, uint32_t index)
{
    int32_t value;
    memcpy(&value, bytes + span->children + (size_t)index * 4, 4);
    return value;
}

int32_t wireChildAt(const uint8_t *bytes, const Span *span, uint32_t index)
{
    return wireChild(bytes, span, index);
}

const Span *wireChildList(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    (void)bytes;
    const Span *found = NULL;
    for (size_t index = 0; index < count; index++) {
        if (spans[index].op == 3 && spans[index].id == id) found = &spans[index];
    }
    return found;
}

int32_t wireParentOf(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    for (size_t index = count; index-- > 0;) {
        const Span *span = &spans[index];
        if (span->op != 3) continue;
        for (uint32_t position = 0; position < span->childCount; position++) {
            if (wireChild(bytes, span, position) == id) return span->id;
        }
    }
    return 0;
}

const Span *wireCreation(const Span *spans, size_t count, int32_t id)
{
    for (size_t index = 0; index < count; index++) {
        if (spans[index].op == 1 && spans[index].id == id) return &spans[index];
    }
    return NULL;
}

int wireStringProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, const char *value)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    size_t valueLength = strlen(value);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 6) {
            uint32_t length = wireRead32(&cursor);
            if (!wireNeed(&cursor, length)) return 0;
            return length == valueLength && memcmp(cursor.bytes + cursor.offset, value, length) == 0;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static void reserve(Out *out, size_t more)
{
    if (out->failed) return;
    if (out->length + more <= out->capacity) return;
    size_t capacity = out->capacity ? out->capacity : 4096;
    while (capacity < out->length + more) capacity *= 2;
    uint8_t *grown = realloc(out->bytes, capacity);
    if (!grown) {
        out->failed = 1;
        return;
    }
    out->bytes = grown;
    out->capacity = capacity;
}

void wirePut(Out *out, const void *bytes, size_t length)
{
    reserve(out, length);
    if (out->failed) return;
    memcpy(out->bytes + out->length, bytes, length);
    out->length += length;
}

void wirePut8(Out *out, uint8_t value)
{
    wirePut(out, &value, 1);
}
void wirePut16(Out *out, uint16_t value)
{
    wirePut(out, &value, 2);
}
void wirePut32(Out *out, uint32_t value)
{
    wirePut(out, &value, 4);
}

void wirePropCount(Out *out, uint16_t count)
{
    wirePut16(out, count);
}
void wirePropString(Out *out, uint16_t property, const char *value)
{
    size_t length = strlen(value);
    wirePut16(out, property);
    wirePut8(out, 6);
    wirePut32(out, (uint32_t)length);
    wirePut(out, value, length);
}
void wirePropNumber(Out *out, uint16_t property, double value)
{
    wirePut16(out, property);
    wirePut8(out, 5);
    wirePut(out, &value, 8);
}
void wirePropInt(Out *out, uint16_t property, int32_t value)
{
    wirePut16(out, property);
    wirePut8(out, 3);
    wirePut32(out, (uint32_t)value);
}
void wirePropLong(Out *out, uint16_t property, int64_t value)
{
    wirePut16(out, property);
    wirePut8(out, 4);
    wirePut(out, &value, 8);
}
void wirePropBool(Out *out, uint16_t property, int value)
{
    wirePut16(out, property);
    wirePut8(out, value ? 2 : 1);
}
void wirePropQuad(Out *out, uint16_t property, float a, float b, float c, float d)
{
    float quad[4] = {a, b, c, d};
    wirePut16(out, property);
    wirePut8(out, 9);
    wirePut(out, quad, sizeof(quad));
}
void wirePropEvents(Out *out, uint16_t property, const char *event)
{
    size_t length = strlen(event);
    wirePut16(out, property);
    wirePut8(out, 7);
    wirePut32(out, 1);
    wirePut8(out, 6);
    wirePut32(out, (uint32_t)length);
    wirePut(out, event, length);
}

void wirePutChildren(Out *out, int32_t parent, const int32_t *children, uint32_t count)
{
    wirePut8(out, 3);
    wirePut32(out, (uint32_t)parent);
    wirePut32(out, count);
    for (uint32_t index = 0; index < count; index++) wirePut32(out, (uint32_t)children[index]);
}

int wireTypeIs(const uint8_t *bytes, const Span *span, const char *type)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    uint16_t length = wireRead16(&cursor);
    return wireNeed(&cursor, length) && length == strlen(type) && memcmp(bytes + cursor.offset, type, length) == 0;
}

void wirePropGradient(Out *out, uint16_t property, double angle, const uint32_t *colors, uint32_t count)
{
    wirePut16(out, property);
    wirePut8(out, 8);
    wirePut32(out, 3);
    wirePut16(out, P(angle));
    wirePut8(out, 5);
    wirePut(out, &angle, 8);
    wirePut16(out, P(colors));
    wirePut8(out, 7);
    wirePut32(out, count);
    for (uint32_t index = 0; index < count; index++) {
        wirePut8(out, 4);
        int64_t color = colors[index];
        wirePut(out, &color, 8);
    }
    wirePut16(out, P(positions));
    wirePut8(out, 7);
    wirePut32(out, count);
    for (uint32_t index = 0; index < count; index++) {
        wirePut8(out, 5);
        double position = count > 1 ? (double)index / (count - 1) : 0;
        wirePut(out, &position, 8);
    }
}

int wireColorProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, uint32_t *color)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && (tag == 3 || tag == 4)) {
            uint32_t value = wireRead32(&cursor);
            if (cursor.failed) return 0;
            *color = value;
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}
