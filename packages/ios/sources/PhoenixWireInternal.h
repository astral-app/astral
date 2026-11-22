#pragma once

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "PhoenixProperties.h"
#include "PhoenixWire.h"

enum { kMaxSpans = 8192, kMaxChildren = 4096, kMaxProps = 128 };
enum { kWireVersion = 1 };

static const int32_t kFirstId = 0x7a000000;

typedef struct {
    uint8_t op;
    int32_t id;
    size_t start;
    size_t end;
    size_t children;
    uint32_t childCount;
} Span;

typedef struct {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
    int failed;
} Cursor;

typedef struct {
    uint8_t *bytes;
    size_t length;
    size_t capacity;
    int failed;
} Out;

static inline int wireNeed(Cursor *cursor, size_t count)
{
    if (cursor->failed || cursor->length - cursor->offset < count) {
        cursor->failed = 1;
        return 0;
    }
    return 1;
}

static inline uint32_t wireRead32(Cursor *cursor)
{
    if (!wireNeed(cursor, 4)) return 0;
    uint32_t value;
    memcpy(&value, cursor->bytes + cursor->offset, 4);
    cursor->offset += 4;
    return value;
}

static inline uint16_t wireRead16(Cursor *cursor)
{
    if (!wireNeed(cursor, 2)) return 0;
    uint16_t value = (uint16_t)(cursor->bytes[cursor->offset] | cursor->bytes[cursor->offset + 1] << 8);
    cursor->offset += 2;
    return value;
}

static inline uint8_t wireRead8(Cursor *cursor)
{
    return wireNeed(cursor, 1) ? cursor->bytes[cursor->offset++] : 0;
}

static inline void wireSkip(Cursor *cursor, size_t count)
{
    if (wireNeed(cursor, count)) cursor->offset += count;
}

typedef struct {
    int32_t id;
    uint8_t kind;
    uint64_t value;
} Tracked;

enum { kTrackNone, kTrackAuthor, kTrackAvatar, kTrackAvatarImage, kTrackChannel, kTrackTimestamp };
enum { kTrackCapacity = 32768, kMaxTags = 4, kMaxDecorated = 4096 };

typedef struct {
    int32_t parent;
    int32_t tags[kMaxTags * 2];
    uint32_t count;
} Decorated;

void wireSkipValue(Cursor *cursor, uint8_t tag, int depth);
int wireScan(const uint8_t *bytes, size_t length, Span *spans, size_t *count);
int32_t wireChild(const uint8_t *bytes, const Span *span, uint32_t index);
int32_t wireChildAt(const uint8_t *bytes, const Span *span, uint32_t index);
const Span *wireChildList(const uint8_t *bytes, const Span *spans, size_t count, int32_t id);
int32_t wireParentOf(const uint8_t *bytes, const Span *spans, size_t count, int32_t id);
const Span *wireCreation(const Span *spans, size_t count, int32_t id);
int wireStringProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, const char *value);
void wirePut(Out *out, const void *bytes, size_t length);
void wirePut8(Out *out, uint8_t value);
void wirePut16(Out *out, uint16_t value);
void wirePut32(Out *out, uint32_t value);
void wirePropCount(Out *out, uint16_t count);
void wirePropString(Out *out, uint16_t property, const char *value);
void wirePropNumber(Out *out, uint16_t property, double value);
void wirePropInt(Out *out, uint16_t property, int32_t value);
void wirePropLong(Out *out, uint16_t property, int64_t value);
void wirePropBool(Out *out, uint16_t property, int value);
void wirePropQuad(Out *out, uint16_t property, float a, float b, float c, float d);
void wirePropEvents(Out *out, uint16_t property, const char *event);
void wirePropGradient(Out *out, uint16_t property, double angle, const uint32_t *colors, uint32_t count);
int wireColorProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, uint32_t *color);
size_t wireBackgroundStops(uint32_t *colors, size_t capacity);
void wireTrackImages(const uint8_t *bytes, const Span *spans, size_t count);
void wireColorNames(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireDecorateMentions(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireSettingsBackdrop(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wirePutChildren(Out *out, int32_t parent, const int32_t *children, uint32_t count);
int wireTypeIs(const uint8_t *bytes, const Span *span, const char *type);
void wireSetTemplateSource(const uint8_t *raw);
void wireRewritePages(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
Tracked *wireTrackSlot(int32_t id, int create);
uint64_t wireAvatarUser(const uint8_t *text, size_t length);
int wirePrefixed(const uint8_t *text, size_t length, const char *prefix, uint64_t *suffix);
int32_t wireBuildTag(Out *out, const AstralTag *tag, Decorated *slot);
void wireDecorate(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireObserveRemovals(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireObserve(const uint8_t *bytes, const Span *spans, size_t count);
int wirePatchColors(uint8_t *bytes, const Span *spans, size_t count);
void wireTrackColors(const uint8_t *bytes, const Span *spans, size_t count);
int wireThemeActive(void);
uint8_t *wireRelabel(const uint8_t *bytes, size_t length, Span *spans, size_t count, size_t *newLength);
