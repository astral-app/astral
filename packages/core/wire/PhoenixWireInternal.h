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

enum { kMaxTags = 4 };

typedef struct {
    int32_t parent;
    int32_t tags[kMaxTags * 2];
    uint32_t count;
    int32_t roots[kMaxTags];
    uint32_t rootCount;
    uint64_t message;
    uint64_t signature;
} Decorated;

typedef enum { kKeep, kDrop, kString, kNumber, kInteger, kBool } Edit;

typedef struct {
    uint16_t property;
    Edit edit;
    const char *text;
    double number;
} Change;

void wireSkipValue(Cursor *cursor, uint8_t tag, int depth);
void wireCloneCreation(Out *out, const uint8_t *bytes, const Span *span, int32_t id, const Change *changes, size_t changeCount);
void wireMessageMenu(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireMessageTexts(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireTweenIn(int32_t node, double width, double inset);
void wireTweenForget(int32_t node);
void wireTweenFade(int32_t node);
int32_t wireFirstOfType(const uint8_t *bytes, const Span *spans, size_t count, int32_t node, const char *type, int depth);
void wireTrackComposer(const uint8_t *bytes, const Span *spans, size_t count);
void wireTrackMessageNodes(const uint8_t *bytes, const Span *spans, size_t count);
int wireScan(const uint8_t *bytes, size_t length, Span *spans, size_t *count);
int32_t wireChild(const uint8_t *bytes, const Span *span, uint32_t index);
int32_t wireChildAt(const uint8_t *bytes, const Span *span, uint32_t index);
const Span *wireChildList(const uint8_t *bytes, const Span *spans, size_t count, int32_t id);
int32_t wireParentOf(const uint8_t *bytes, const Span *spans, size_t count, int32_t id);
const Span *wireCreation(const Span *spans, size_t count, int32_t id);
int wireStringProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, const char *value);
int wireStringPropertyPrefix(const uint8_t *bytes, const Span *span, uint16_t wanted, const char *prefix);
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
void wireReactionEmoji(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireGlyphOf(const char *codepoints, char *glyph, size_t capacity);
void wireReactionAvatars(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
uint8_t *wireEmojiBatch(const uint8_t *bytes, size_t length, Span *spans, size_t *count, size_t *newLength);
void wireColorNames(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireDecorateMentions(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireObserveAppGradient(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wirePutChildren(Out *out, int32_t parent, const int32_t *children, uint32_t count);
int wireTypeIs(const uint8_t *bytes, const Span *span, const char *type);
void wireSetTemplateSource(const uint8_t *raw);
void wireRewritePages(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
uint64_t wireAvatarUser(const uint8_t *text, size_t length);
void wireSetCurrentChannel(uint64_t channel);
int wirePrefixed(const uint8_t *text, size_t length, const char *prefix, uint64_t *suffix);
int32_t wireBuildTag(Out *out, const AstralTag *tag, Decorated *slot);
void wireMarkRefreshBusy(void);
int wirePatchingActive(void);
int32_t wireAllocateNode(void);
uint64_t wireRowMessage(int32_t row);
enum { kWireIdentifierLength = 128, kWireTypeLength = 16 };
int wireReadIdentifier(const uint8_t *bytes, const Span *span, char *identifier, char *type);
uint64_t wireMessageOfIdentifier(const char *identifier);
int wireWriteHostLayout(Out *out, int32_t parent);
void wireApplyMessageText(Out *out, uint64_t message);
uint32_t wireInsertedChildren(int32_t parent, int32_t *children, uint32_t capacity, int *atStart);
void wireInsertNodes(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
uint64_t wireMessageAuthor(uint64_t message);
void wirePatchNodes(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireDecorate(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireObserveRemovals(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length);
void wireObserve(const uint8_t *bytes, const Span *spans, size_t count);
int wirePatchColors(uint8_t *bytes, const Span *spans, size_t count);
void wireTrackColors(const uint8_t *bytes, const Span *spans, size_t count);
int wireThemeActive(void);
int wireRowOfMessage(uint64_t message, int32_t *list, int64_t *index);
int32_t wireComposerNode(void);
uint8_t *wireRewriteImages(const uint8_t *bytes, size_t length, Span *spans, size_t *count, size_t *newLength);
uint8_t *wireExpandChildEdits(const uint8_t *bytes, size_t length, Span *spans, size_t *count, size_t *newLength);
uint32_t wireDiscordChildren(int32_t parent, const int32_t **children);
int32_t wireDiscordParent(int32_t child);
void wireResetChildLists(void);
uint8_t *wireRelabel(const uint8_t *bytes, size_t length, Span *spans, size_t count, size_t *newLength);
