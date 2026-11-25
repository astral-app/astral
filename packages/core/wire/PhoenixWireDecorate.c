#include "PhoenixWireInternal.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdatomic.h>
#include <string.h>

static const int32_t kFirstTagId = 0x7b000000;

static Tracked gTracked[kTrackCapacity];
static uint64_t gMessageAuthors[kTrackCapacity][2];
static AstralDecorator gDecorator;
static AstralMessageBackgroundDecorator gMessageBackgroundDecorator;
static atomic_uint_fast64_t gCurrentChannel;
static int32_t gNextTagId = kFirstTagId;

static Decorated gDecorated[kMaxDecorated];

enum {
    kBackgroundLinkCapacity = 16384,
    kBackgroundRowCapacity = 512,
    kBackgroundRowMapCapacity = 1024,
    kBubbleChildren = 4,
    kBubbleBlocks = 6,
    kMediaCapacity = 512,
    kInsideCapacity = 256
};

static const double kIconTagSize = 16;
static const char kContentPrefix[] = "message.content:";
static const float kBubblePadding[4] = {14, 10, 14, 10};
static const float kMediaBlockPadding[4] = {0, 8, 0, 0};
static const float kMediaInset = 4;
static const float kReplyPadding[4] = {8, 3, 8, 3};
static const float kAvatarDecorationInset = 4;
static const double kSquaredCornerRatio = 0.3;
enum { kCornerTopLeft = 1, kCornerTopRight = 2, kCornerBottomLeft = 4, kCornerBottomRight = 8, kCornersAll = 15 };
static const double kReplyRadius = 9;
static const float kQuotePadding[4] = {8, 4, 8, 4};
static const uint32_t kQuoteColor = 0x26000000;

typedef struct {
    int32_t id;
    int32_t parent;
} BackgroundLink;

typedef struct {
    int32_t id;
    uint64_t message;
    uint64_t user;
    int32_t content;
} BackgroundRow;

typedef struct {
    int32_t id;
    int media;
    int32_t card;
    uint32_t cardColor;
    double cardRadius;
    int32_t accent;
    uint32_t accentColor;
    float padding[4];
    int32_t children[kBubbleChildren];
    uint32_t childCount;
} BubbleBlock;

typedef struct {
    int32_t id;
    uint64_t message;
    uint64_t user;
    uint64_t channel;
    uint32_t originalColor;
    uint32_t appliedColor;
    double originalRadius;
    double appliedRadius;
    int hasOriginal;
    int active;
    int bubble;
    int replyShown;
    int32_t column;
    int32_t reply;
    int32_t replyBar;
    int32_t barChildren[kBubbleChildren];
    uint32_t barChildCount;
    float barPadding[4];
    float barMargin[4];
    int32_t quotedIn;
    int32_t quoteBox;
    int32_t header;
    int32_t timestamp;
    int32_t timeParent;
    int32_t timeParentChildren[kBubbleChildren * 2];
    uint32_t timeParentChildCount;
    char timeText[48];
    int32_t columnChildren[kBubbleChildren * 2];
    uint32_t columnChildCount;
    int nameShown;
    int32_t avatar;
    float avatarMargin[4];
    int avatarShifted;
    int corners;
    int timeShown;
    BubbleBlock blocks[kBubbleBlocks];
    uint32_t blockCount;
    uint32_t shownBlocks;
} BackgroundState;

static BackgroundState gBackgroundStates[kMaxDecorated];

void astral_wire_set_decorator(AstralDecorator decorator)
{
    gDecorator = decorator;
}

void astral_wire_set_message_background_decorator(AstralMessageBackgroundDecorator decorator)
{
    gMessageBackgroundDecorator = decorator;
}

static AstralWireStats gStats;

AstralWireStats astral_wire_stats(void)
{
    return gStats;
}

uint64_t astral_wire_current_channel(void)
{
    return atomic_load_explicit(&gCurrentChannel, memory_order_relaxed);
}

Tracked *wireTrackSlot(int32_t id, int create)
{
    uint32_t index = ((uint32_t)id * 2654435761u) % kTrackCapacity;
    for (uint32_t probe = 0; probe < kTrackCapacity; probe++) {
        Tracked *slot = &gTracked[(index + probe) % kTrackCapacity];
        if (slot->id == id) return slot;
        if (slot->id == 0) {
            if (!create) return NULL;
            slot->id = id;
            return slot;
        }
    }
    return NULL;
}

static void forget(int32_t id)
{
    Tracked *slot = wireTrackSlot(id, 0);
    if (slot) slot->kind = kTrackNone;
}

static void rememberAuthor(uint64_t message, uint64_t user)
{
    uint32_t index = (uint32_t)(message % kTrackCapacity);
    gMessageAuthors[index][0] = message;
    gMessageAuthors[index][1] = user;
}

static uint64_t authorOf(uint64_t message)
{
    uint32_t index = (uint32_t)(message % kTrackCapacity);
    return gMessageAuthors[index][0] == message ? gMessageAuthors[index][1] : 0;
}

uint64_t wireMessageAuthor(uint64_t message)
{
    return authorOf(message);
}

static uint64_t parseId(const uint8_t *text, size_t length)
{
    uint64_t value = 0;
    size_t digits = 0;
    for (size_t index = 0; index < length && text[index] >= '0' && text[index] <= '9'; index++, digits++) {
        value = value * 10 + (uint64_t)(text[index] - '0');
    }
    return digits >= 15 ? value : 0;
}

uint64_t wireAvatarUser(const uint8_t *text, size_t length)
{
    static const char *markers[] = {"/avatars/", "/users/"};
    for (size_t marker = 0; marker < 2; marker++) {
        size_t markerLength = strlen(markers[marker]);
        for (size_t index = 0; index + markerLength < length; index++) {
            if (memcmp(text + index, markers[marker], markerLength) == 0) {
                uint64_t user = parseId(text + index + markerLength, length - index - markerLength);
                if (user) return user;
            }
        }
    }
    return 0;
}

int wirePrefixed(const uint8_t *text, size_t length, const char *prefix, uint64_t *suffix)
{
    size_t prefixLength = strlen(prefix);
    if (length <= prefixLength || memcmp(text, prefix, prefixLength) != 0) return 0;
    *suffix = parseId(text + prefixLength, length - prefixLength);
    return *suffix != 0;
}

static void observeProperties(const uint8_t *bytes, const Span *span)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    int selected = -1;
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (tag == 6 && (property == P(accessibilityId) || property == P(uri))) {
            uint32_t length = wireRead32(&cursor);
            if (!wireNeed(&cursor, length)) return;
            const uint8_t *text = bytes + cursor.offset;
            cursor.offset += length;
            uint64_t value = 0;
            int kind = kTrackNone;
            if (property == P(accessibilityId) && wirePrefixed(text, length, "message.author:", &value)) {
                kind = kTrackAuthor;
            } else if (property == P(accessibilityId) && wirePrefixed(text, length, "message.avatar:", &value)) {
                kind = kTrackAvatar;
            } else if (property == P(accessibilityId) && wirePrefixed(text, length, "message.timestamp:", &value)) {
                kind = kTrackTimestamp;
            } else if (property == P(accessibilityId) && wirePrefixed(text, length, "navigation.channel:", &value)) {
                kind = kTrackChannel;
            } else if (property == P(uri) && (value = wireAvatarUser(text, length))) {
                kind = kTrackAvatarImage;
            }
            Tracked *slot = kind == kTrackNone ? NULL : wireTrackSlot(span->id, 1);
            if (slot) *slot = (Tracked){span->id, kind, value};
            continue;
        }
        if (property == P(accessibilitySelected) && (tag == 1 || tag == 2)) {
            selected = tag == 2;
            continue;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    Tracked *slot = selected >= 0 ? wireTrackSlot(span->id, 0) : NULL;
    if (slot && slot->kind == kTrackChannel) {
        uint64_t current = atomic_load_explicit(&gCurrentChannel, memory_order_relaxed);
        if (selected == 1)
            atomic_store_explicit(&gCurrentChannel, slot->value, memory_order_relaxed);
        else if (current == slot->value)
            atomic_store_explicit(&gCurrentChannel, 0, memory_order_relaxed);
    }
}

static void propagateAvatars(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (int pass = 0; pass < 3; pass++) {
        for (size_t index = 0; index < count; index++) {
            const Span *span = &spans[index];
            if (span->op != 3 || span->childCount == 0 || span->childCount > 4) continue;
            for (uint32_t position = 0; position < span->childCount; position++) {
                Tracked *child = wireTrackSlot(wireChildAt(bytes, span, position), 0);
                if (!child || child->kind != kTrackAvatarImage) continue;
                Tracked *holder = wireTrackSlot(span->id, 1);
                if (!holder) break;
                if (holder->kind == kTrackAvatar) {
                    rememberAuthor(holder->value, child->value);
                } else if (holder->kind == kTrackNone || holder->kind == kTrackAvatarImage) {
                    *holder = (Tracked){span->id, kTrackAvatarImage, child->value};
                }
                break;
            }
        }
    }
}

static Decorated *decoratedSlot(int32_t parent, int create)
{
    uint32_t index = ((uint32_t)parent * 2654435761u) % kMaxDecorated;
    for (uint32_t probe = 0; probe < kMaxDecorated; probe++) {
        Decorated *slot = &gDecorated[(index + probe) % kMaxDecorated];
        if (slot->parent == parent) return slot;
        if (slot->parent == 0) {
            if (!create) return NULL;
            slot->parent = parent;
            slot->count = 0;
            return slot;
        }
    }
    return NULL;
}

static void removeTags(Out *out, Decorated *slot)
{
    if (!slot || !slot->count) return;
    wirePut8(out, 4);
    wirePut32(out, slot->count);
    for (uint32_t index = 0; index < slot->count; index++) wirePut32(out, (uint32_t)slot->tags[index]);
    slot->count = 0;
}

static size_t backgroundIndex(int32_t id, size_t capacity)
{
    return ((uint32_t)id * 2654435761u) % capacity;
}

static void backgroundPutLink(BackgroundLink *links, int32_t id, int32_t parent)
{
    if (!id) return;
    size_t index = backgroundIndex(id, kBackgroundLinkCapacity);
    for (size_t probe = 0; probe < kBackgroundLinkCapacity; probe++) {
        BackgroundLink *entry = &links[(index + probe) % kBackgroundLinkCapacity];
        if (entry->id == id || entry->id == 0) {
            *entry = (BackgroundLink){id, parent};
            return;
        }
    }
}

static int32_t backgroundParent(const BackgroundLink *links, int32_t id)
{
    if (!id) return 0;
    size_t index = backgroundIndex(id, kBackgroundLinkCapacity);
    for (size_t probe = 0; probe < kBackgroundLinkCapacity; probe++) {
        const BackgroundLink *entry = &links[(index + probe) % kBackgroundLinkCapacity];
        if (entry->id == id) return entry->parent;
        if (!entry->id) return 0;
    }
    return 0;
}

static void backgroundPutRow(int32_t *rows, int32_t id)
{
    if (!id) return;
    size_t index = backgroundIndex(id, kBackgroundRowMapCapacity);
    for (size_t probe = 0; probe < kBackgroundRowMapCapacity; probe++) {
        int32_t *entry = &rows[(index + probe) % kBackgroundRowMapCapacity];
        if (*entry == 0 || *entry == id) {
            *entry = id;
            return;
        }
    }
}

static int32_t backgroundRow(const int32_t *rows, int32_t id)
{
    if (!id) return 0;
    size_t index = backgroundIndex(id, kBackgroundRowMapCapacity);
    for (size_t probe = 0; probe < kBackgroundRowMapCapacity; probe++) {
        int32_t entry = rows[(index + probe) % kBackgroundRowMapCapacity];
        if (entry == 0) return 0;
        if (entry == id) return id;
    }
    return 0;
}

static BackgroundState *backgroundState(int32_t id, int create)
{
    if (!id) return NULL;
    size_t index = backgroundIndex(id, kMaxDecorated);
    BackgroundState *available = NULL;
    for (size_t probe = 0; probe < kMaxDecorated; probe++) {
        BackgroundState *entry = &gBackgroundStates[(index + probe) % kMaxDecorated];
        if (entry->id == id) return entry;
        if (!entry->hasOriginal && !available) available = entry;
        if (!entry->id) {
            available = entry;
            break;
        }
    }
    if (!create || !available) return NULL;
    memset(available, 0, sizeof(*available));
    available->id = id;
    return available;
}

static int numberProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, double *value)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 5) {
            if (!wireNeed(&cursor, 8)) return 0;
            memcpy(value, bytes + cursor.offset, 8);
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static int validBackgroundStyle(const AstralMessageBackgroundStyle *style)
{
    return isfinite(style->radius) && style->radius >= 0 && style->radius <= 32;
}

static void putUnsetWidth(Out *out, int32_t node, int fill)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)node);
    wirePropCount(out, 1);
    if (fill) {
        wirePropString(out, P(width), "fill");
    } else {
        wirePut16(out, P(width));
        wirePut8(out, 0);
    }
}

static void writeCardUpdate(Out *out, const BubbleBlock *block, int shown, uint32_t color)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)block->card);
    wirePropCount(out, 2);
    wirePropLong(out, P(background), shown ? (int64_t)color : (int64_t)block->cardColor);
    wirePropNumber(out, P(radius), block->cardRadius);
    if (!block->accent) return;
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)block->accent);
    wirePropCount(out, 1);
    wirePropLong(out, P(background), shown ? 0 : (int64_t)block->accentColor);
}

static void writeBlockUpdate(Out *out, const BubbleBlock *block, int shown, uint32_t color, double radius)
{
    if (block->card) {
        writeCardUpdate(out, block, shown, color);
        return;
    }
    const float *padding = block->media ? block->padding : (const float[4]){0, 0, 0, 0};
    double blockRadius = radius;
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)block->id);
    wirePropCount(out, block->media ? 5 : 4);
    wirePropLong(out, P(background), shown ? (int64_t)color : 0);
    wirePropNumber(out, P(radius), shown ? blockRadius : 0);
    if (!shown)
        wirePropQuad(out, P(padding), padding[0], padding[1], padding[2], padding[3]);
    else if (block->media)
        wirePropQuad(out, P(padding), kMediaInset, kMediaInset, kMediaInset, kMediaInset);
    else
        wirePropQuad(out, P(padding), kBubblePadding[0], kBubblePadding[1], kBubblePadding[2], kBubblePadding[3]);
    if (block->media) wirePropQuad(out, P(margin), 0, shown ? block->padding[1] - kMediaInset : 0, 0, 0);
    if (shown) {
        wirePut16(out, P(width));
        wirePut8(out, 0);
    } else {
        wirePropString(out, P(width), "fill");
    }
    for (uint32_t index = 0; index < block->childCount; index++) putUnsetWidth(out, block->children[index], !shown);
}

static void putCorner(Out *out, uint16_t property, int shown, int rounded, double radius)
{
    if (!shown) {
        wirePut16(out, property);
        wirePut8(out, 0);
        return;
    }
    wirePropNumber(out, property, rounded ? radius : radius * kSquaredCornerRatio);
}

static void writeCorners(Out *out, const BackgroundState *state, int shown, double radius)
{
    int corners = state->corners ? state->corners : kCornersAll;
    for (uint32_t index = 0; index < state->blockCount; index++) {
        const BubbleBlock *block = &state->blocks[index];
        if (block->card) continue;
        wirePut8(out, 2);
        wirePut32(out, (uint32_t)block->id);
        wirePropCount(out, 4);
        putCorner(out, P(radiusTopLeft), shown, corners & kCornerTopLeft, radius);
        putCorner(out, P(radiusTopRight), shown, corners & kCornerTopRight, radius);
        putCorner(out, P(radiusBottomLeft), shown, corners & kCornerBottomLeft, radius);
        putCorner(out, P(radiusBottomRight), shown, corners & kCornerBottomRight, radius);
    }
}

static void writeBubbleUpdate(Out *out, const BackgroundState *state, int shown, uint32_t color, double radius)
{
    for (uint32_t index = 0; index < state->blockCount; index++) writeBlockUpdate(out, &state->blocks[index], shown, color, radius);
    writeCorners(out, state, shown, radius);
}

static void writePreviewStyle(Out *out, int32_t preview, int mode, uint32_t color)
{
    static const float none[4] = {0, 0, 0, 0};
    const float *padding = mode == AstralReplyBubble ? kReplyPadding : none;
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)preview);
    wirePropCount(out, 3);
    wirePropLong(out, P(background), mode == AstralReplyBubble ? (int64_t)color : 0);
    wirePropNumber(out, P(radius), mode == AstralReplyBubble ? kReplyRadius : 0);
    wirePropQuad(out, P(padding), padding[0], padding[1], padding[2], padding[3]);
}

static void writeBackgroundUpdate(Out *out, const BackgroundState *state, uint32_t color, double radius)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)state->id);
    wirePropCount(out, 2);
    wirePropLong(out, P(background), (int64_t)color);
    wirePropNumber(out, P(radius), radius);
}

static int32_t backgroundRowForNode(const BackgroundLink *links, const int32_t *rows, int32_t id)
{
    for (int depth = 0; depth < 24 && id; depth++) {
        int32_t row = backgroundRow(rows, id);
        if (row) return row;
        id = backgroundParent(links, id);
    }
    return 0;
}

static size_t backgroundRowIndex(const BackgroundRow *rows, size_t count, int32_t id)
{
    for (size_t index = 0; index < count; index++) {
        if (rows[index].id == id) return index;
    }
    return count;
}

static int backgroundWasRemoved(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 4) continue;
        for (uint32_t position = 0; position < span->childCount; position++) {
            if (wireChildAt(bytes, span, position) == id) return 1;
        }
    }
    return 0;
}

static void captureOriginalBackground(const uint8_t *bytes, const Span *creation, BackgroundState *state)
{
    state->originalColor = 0;
    state->originalRadius = 0;
    if (creation) {
        wireColorProperty(bytes, creation, P(background), &state->originalColor);
        numberProperty(bytes, creation, P(radius), &state->originalRadius);
    }
    state->hasOriginal = 1;
}

static int quadProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, float quad[4])
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 9) {
            if (!wireNeed(&cursor, sizeof(float) * 4)) return 0;
            memcpy(quad, bytes + cursor.offset, sizeof(float) * 4);
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static int isMediaBlock(const uint8_t *bytes, const Span *span)
{
    float padding[4];
    return span->op == 1 && wireTypeIs(bytes, span, "view") && wireStringProperty(bytes, span, P(width), "fill") &&
           quadProperty(bytes, span, P(padding), padding) && padding[0] == 0 && padding[2] == 0 && padding[3] == 0 && padding[1] > 0 &&
           padding[1] <= 12;
}

static BubbleBlock *addBlock(BackgroundState *state, int32_t id, int media)
{
    for (uint32_t index = 0; index < state->blockCount; index++) {
        if (state->blocks[index].id == id) return &state->blocks[index];
    }
    if (state->blockCount >= kBubbleBlocks) return NULL;
    BubbleBlock *block = &state->blocks[state->blockCount++];
    *block = (BubbleBlock){.id = id, .media = media};
    memcpy(block->padding, kMediaBlockPadding, sizeof(block->padding));
    return block;
}

static int32_t childOf(const uint8_t *bytes, const Span *spans, size_t count, int32_t parent, uint32_t position)
{
    const Span *list = wireChildList(bytes, spans, count, parent);
    return list && position < list->childCount ? wireChildAt(bytes, list, position) : 0;
}

static int isDarkOpaque(uint32_t color)
{
    uint32_t red = (color >> 16) & 0xFF, green = (color >> 8) & 0xFF, blue = color & 0xFF;
    return (color >> 24) == 0xFF && red * 299 + green * 587 + blue * 114 < 64 * 1000;
}

static void rememberDefaultAccent(const uint8_t *bytes, const Span *spans, size_t count, BubbleBlock *block)
{
    block->accent = 0;
    int32_t node = block->card;
    for (int depth = 0; depth < 2; depth++) {
        node = childOf(bytes, spans, count, node, 0);
        const Span *creation = wireCreation(spans, count, node);
        double width = 0;
        uint32_t color = 0;
        if (!creation) return;
        if (!numberProperty(bytes, creation, P(width), &width) || width > 6) continue;
        if (!wireColorProperty(bytes, creation, P(background), &color) || !isDarkOpaque(color)) return;
        block->accent = node;
        block->accentColor = color;
        return;
    }
}

static void rememberBlockChildren(const uint8_t *bytes, const Span *spans, size_t count, BubbleBlock *block)
{
    const Span *list = wireChildList(bytes, spans, count, block->id);
    if (!list) return;
    block->childCount = 0;
    for (uint32_t position = 0; position < list->childCount && block->childCount < kBubbleChildren; position++) {
        int32_t child = wireChildAt(bytes, list, position);
        const Span *creation = wireCreation(spans, count, child);
        uint32_t cardColor = 0;
        if (block->media && position == 0 && creation && wireColorProperty(bytes, creation, P(background), &cardColor) && cardColor) {
            block->card = child;
            block->cardColor = cardColor;
            block->cardRadius = 0;
            numberProperty(bytes, creation, P(radius), &block->cardRadius);
            rememberDefaultAccent(bytes, spans, count, block);
            return;
        }
        if (block->media && !(creation && wireStringProperty(bytes, creation, P(width), "fill"))) continue;
        block->children[block->childCount++] = child;
    }
}

static int32_t replyBody(const uint8_t *bytes, const Span *spans, size_t count, int32_t row)
{
    for (uint32_t position = 0; position < 2; position++) {
        int32_t child = childOf(bytes, spans, count, row, position);
        const Span *creation = wireCreation(spans, count, child);
        if (creation && wireTypeIs(bytes, creation, "view") && wireStringProperty(bytes, creation, P(direction), "column")) return child;
    }
    return 0;
}

static int32_t replyBar(const uint8_t *bytes, const Span *spans, size_t count, int32_t body)
{
    int32_t bar = childOf(bytes, spans, count, body, 0);
    const Span *list = wireChildList(bytes, spans, count, bar);
    if (list && list->childCount == 1) return wireChildAt(bytes, list, 0);
    return bar;
}

static void rememberReply(const uint8_t *bytes, const Span *spans, size_t count, BackgroundState *state, int32_t row)
{
    int32_t body = replyBody(bytes, spans, count, row);
    if (!body) return;
    int32_t bar = replyBar(bytes, spans, count, body);
    const Span *barCreation = wireCreation(spans, count, bar);
    const Span *barList = wireChildList(bytes, spans, count, bar);
    int32_t preview = childOf(bytes, spans, count, bar, 1);
    const Span *previewCreation = wireCreation(spans, count, preview);
    if (!barCreation || !barList || !previewCreation || !wireTypeIs(bytes, previewCreation, "stack")) return;
    state->reply = preview;
    state->replyBar = bar;
    state->barChildCount = 0;
    for (uint32_t position = 0; position < barList->childCount && state->barChildCount < kBubbleChildren; position++)
        state->barChildren[state->barChildCount++] = wireChildAt(bytes, barList, position);
    memset(state->barPadding, 0, sizeof(state->barPadding));
    memset(state->barMargin, 0, sizeof(state->barMargin));
    quadProperty(bytes, barCreation, P(padding), state->barPadding);
    quadProperty(bytes, barCreation, P(margin), state->barMargin);
}

static const BubbleBlock *findBlock(const BackgroundState *state, int32_t id)
{
    for (uint32_t index = 0; index < state->blockCount; index++) {
        if (state->blocks[index].id == id) return &state->blocks[index];
    }
    return NULL;
}

static const BubbleBlock *textBlock(const BackgroundState *state)
{
    for (uint32_t index = 0; index < state->blockCount; index++) {
        if (!state->blocks[index].media) return &state->blocks[index];
    }
    return NULL;
}

static int32_t tagId(void);

static int32_t createQuoteBox(Out *out, double radius)
{
    int32_t box = tagId();
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)box);
    wirePut16(out, 4);
    wirePut(out, "view", 4);
    wirePropCount(out, 6);
    wirePropString(out, P(direction), "column");
    wirePropString(out, P(align), "start");
    wirePropLong(out, P(background), kQuoteColor);
    wirePropNumber(out, P(radius), fmax(radius - 4, 4));
    wirePropQuad(out, P(padding), kQuotePadding[0], kQuotePadding[1], kQuotePadding[2], kQuotePadding[3]);
    wirePropQuad(out, P(margin), 0, 0, 0, 6);
    return box;
}

static void writeBarLayout(Out *out, const BackgroundState *state, int inside)
{
    static const float none[4] = {0, 0, 0, 0};
    wirePutChildren(out, state->replyBar, state->barChildren, inside ? 0 : state->barChildCount);
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)state->replyBar);
    wirePropCount(out, 2);
    const float *padding = inside ? none : state->barPadding;
    const float *margin = inside ? none : state->barMargin;
    wirePropQuad(out, P(padding), padding[0], padding[1], padding[2], padding[3]);
    wirePropQuad(out, P(margin), margin[0], margin[1], margin[2], margin[3]);
}

static void writeTextDirection(Out *out, int32_t text, int inside)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)text);
    wirePropCount(out, 1);
    if (inside) {
        wirePropString(out, P(direction), "column");
    } else {
        wirePut16(out, P(direction));
        wirePut8(out, 0);
    }
}

static uint32_t leadingChildren(const BackgroundState *state, int32_t *children)
{
    uint32_t count = 0;
    if (state->nameShown && state->header) children[count++] = state->header;
    if (state->quoteBox) children[count++] = state->quoteBox;
    return count;
}

static void writeHostLayout(Out *out, const BackgroundState *state, const BubbleBlock *text)
{
    int32_t children[2 + kBubbleChildren + kAstralInsertCapacity];
    int32_t inserted[kAstralInsertCapacity];
    int insertedAtStart = 0;
    uint32_t insertedCount = wireInsertedChildren(text->id, inserted, kAstralInsertCapacity, &insertedAtStart);
    uint32_t leading = leadingChildren(state, children);
    uint32_t childCount = leading;
    if (insertedAtStart)
        for (uint32_t index = 0; index < insertedCount; index++) children[childCount++] = inserted[index];
    for (uint32_t index = 0; index < text->childCount; index++) children[childCount++] = text->children[index];
    if (!insertedAtStart)
        for (uint32_t index = 0; index < insertedCount; index++) children[childCount++] = inserted[index];
    wirePutChildren(out, text->id, children, childCount);
    writeTextDirection(out, text->id, leading > 0);
}

static void writeColumnLayout(Out *out, const BackgroundState *state)
{
    if (!state->column || !state->columnChildCount) return;
    int32_t children[kBubbleChildren * 2];
    uint32_t count = 0;
    for (uint32_t index = 0; index < state->columnChildCount; index++) {
        if (state->nameShown && state->columnChildren[index] == state->header) continue;
        children[count++] = state->columnChildren[index];
    }
    wirePutChildren(out, state->column, children, count);
}

static void writeQuoteMove(Out *out, BackgroundState *state, const BubbleBlock *text, int inside, double radius)
{
    if (inside && !state->quoteBox) state->quoteBox = createQuoteBox(out, radius);
    writeBarLayout(out, state, inside);
    if (inside) wirePutChildren(out, state->quoteBox, &state->reply, 1);
    int32_t box = state->quoteBox;
    if (!inside) state->quoteBox = 0;
    writeHostLayout(out, state, text);
    if (inside || !box) return;
    wirePutChildren(out, box, NULL, 0);
    wirePut8(out, 4);
    wirePut32(out, 1);
    wirePut32(out, (uint32_t)box);
}

static void writeTimeParentLayout(Out *out, const BackgroundState *state)
{
    int32_t children[kBubbleChildren * 2];
    uint32_t count = 0;
    for (uint32_t index = 0; index < state->timeParentChildCount; index++) {
        if (state->timeShown != AstralTimeHeader && state->timeParentChildren[index] == state->timestamp) continue;
        children[count++] = state->timeParentChildren[index];
    }
    wirePutChildren(out, state->timeParent, children, count);
}

static int canMoveTime(const BackgroundState *state)
{
    return state->timestamp && state->timeParent && state->timeParentChildCount;
}

static void trackInside(int32_t row, int inside);
static void writeAvatarShift(Out *out, BackgroundState *state, int shifted);

static void writeHeaderLayout(Out *out, BackgroundState *state, const BubbleBlock *text, int name, int time)
{
    state->nameShown = name;
    state->timeShown = time;
    writeColumnLayout(out, state);
    writeAvatarShift(out, state, name);
    if (text) writeHostLayout(out, state, text);
    if (canMoveTime(state)) writeTimeParentLayout(out, state);
    trackInside(state->id, state->replyShown == AstralReplyInside || name || time != AstralTimeHeader);
    if (state->message) wireApplyMessageText(out, state->message);
}

static uint64_t contentMessage(const uint8_t *bytes, const Span *spans, size_t count, int32_t content)
{
    const Span *creation = wireCreation(spans, count, content);
    char identifier[kWireIdentifierLength], type[kWireTypeLength];
    if (!creation || !wireReadIdentifier(bytes, creation, identifier, type)) return 0;
    return wireMessageOfIdentifier(identifier);
}

static void putOriginalOnce(Out *out, const uint8_t *bytes, size_t length, int *changed)
{
    if (!*changed) wirePut(out, bytes, length);
    *changed = 1;
}

static int32_t gInsideRows[kInsideCapacity];

static BackgroundState *hostState(int32_t parent)
{
    for (size_t slot = 0; slot < kInsideCapacity; slot++) {
        BackgroundState *state = gInsideRows[slot] ? backgroundState(gInsideRows[slot], 0) : NULL;
        if (state && state->quotedIn == parent) return state;
    }
    return NULL;
}

size_t wireInlineTime(uint64_t message, char *text, size_t capacity)
{
    for (size_t slot = 0; slot < kInsideCapacity; slot++) {
        BackgroundState *state = gInsideRows[slot] ? backgroundState(gInsideRows[slot], 0) : NULL;
        if (!state || state->message != message || state->timeShown != AstralTimeInline || !state->timeText[0]) continue;
        size_t length = strlen(state->timeText);
        if (length >= capacity) return 0;
        memcpy(text, state->timeText, length + 1);
        return length;
    }
    return 0;
}

int wireWriteHostLayout(Out *out, int32_t parent)
{
    BackgroundState *state = hostState(parent);
    const BubbleBlock *text = state ? findBlock(state, parent) : NULL;
    if (!text) return 0;
    writeHostLayout(out, state, text);
    return 1;
}

static void trackInside(int32_t row, int inside)
{
    for (size_t index = 0; index < kInsideCapacity; index++) {
        if (gInsideRows[index] == row) {
            if (!inside) gInsideRows[index] = 0;
            return;
        }
    }
    if (!inside) return;
    for (size_t index = 0; index < kInsideCapacity; index++) {
        if (!gInsideRows[index]) {
            gInsideRows[index] = row;
            return;
        }
    }
}

static void writeReplyStyle(Out *out, BackgroundState *state, int mode, uint32_t color, double radius)
{
    if (state->replyShown == AstralReplyInside && mode != AstralReplyInside) {
        const BubbleBlock *quoted = findBlock(state, state->quotedIn);
        if (quoted) writeQuoteMove(out, state, quoted, 0, radius);
        if (!state->nameShown && state->timeShown != AstralTimeInline) state->quotedIn = 0;
    }
    if (mode == AstralReplyInside) {
        const BubbleBlock *text = textBlock(state);
        writeQuoteMove(out, state, text, 1, radius);
        state->quotedIn = text->id;
    }
    writePreviewStyle(out, state->reply, mode, color);
    state->replyShown = mode;
    trackInside(state->id, mode == AstralReplyInside || state->nameShown || state->timeShown != AstralTimeHeader);
}

static void rememberColumnChildren(const uint8_t *bytes, const Span *list, BackgroundState *state)
{
    state->columnChildCount = 0;
    for (uint32_t position = 0; position < list->childCount && state->columnChildCount < kBubbleChildren * 2; position++)
        state->columnChildren[state->columnChildCount++] = wireChildAt(bytes, list, position);
}

static void readTextProperty(const uint8_t *bytes, const Span *span, char *text, size_t capacity)
{
    text[0] = 0;
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == P(text) && tag == 6) {
            uint32_t length = wireRead32(&cursor);
            if (!wireNeed(&cursor, length) || length >= capacity) return;
            memcpy(text, bytes + cursor.offset, length);
            text[length] = 0;
            return;
        }
        wireSkipValue(&cursor, tag, 0);
    }
}

static void rememberTimeParentChildren(const uint8_t *bytes, const Span *list, BackgroundState *state)
{
    state->timeParentChildCount = 0;
    for (uint32_t position = 0; position < list->childCount && state->timeParentChildCount < kBubbleChildren * 2; position++)
        state->timeParentChildren[state->timeParentChildCount++] = wireChildAt(bytes, list, position);
}

static void rememberTimestamp(const uint8_t *bytes, const Span *spans, size_t count, const BackgroundLink *links, BackgroundState *state)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        Tracked *tracked = span->op == 1 ? wireTrackSlot(span->id, 0) : NULL;
        if (!tracked || tracked->kind != kTrackTimestamp) continue;
        int32_t parent = backgroundParent(links, span->id);
        int32_t ancestor = parent;
        for (int depth = 0; depth < 6 && ancestor && ancestor != state->header; depth++) ancestor = backgroundParent(links, ancestor);
        const Span *list = parent ? wireChildList(bytes, spans, count, parent) : NULL;
        if (ancestor != state->header || !list) continue;
        state->timestamp = span->id;
        state->timeParent = parent;
        readTextProperty(bytes, span, state->timeText, sizeof(state->timeText));
        rememberTimeParentChildren(bytes, list, state);
        return;
    }
}

static void rememberAvatar(const uint8_t *bytes, const Span *spans, size_t count, const BackgroundLink *links, const int32_t *rowMap,
                           BackgroundState *state)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        Tracked *tracked = span->op == 1 ? wireTrackSlot(span->id, 0) : NULL;
        if (!tracked || tracked->kind != kTrackAvatar || backgroundRowForNode(links, rowMap, span->id) != state->id) continue;
        state->avatar = span->id;
        memset(state->avatarMargin, 0, sizeof(state->avatarMargin));
        quadProperty(bytes, span, P(margin), state->avatarMargin);
        return;
    }
}

static void writeAvatarShift(Out *out, BackgroundState *state, int shifted)
{
    if (!state->avatar || shifted == state->avatarShifted) return;
    const float *margin = state->avatarMargin;
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)state->avatar);
    wirePropCount(out, 1);
    wirePropQuad(out, P(margin), margin[0], margin[1] + (shifted ? kAvatarDecorationInset : 0), margin[2], margin[3]);
    state->avatarShifted = shifted;
}

static void rememberHeader(const uint8_t *bytes, const Span *spans, size_t count, const BackgroundLink *links, const int32_t *authors,
                           size_t authorCount, BackgroundState *state)
{
    if (!state->column) return;
    const Span *columnList = wireChildList(bytes, spans, count, state->column);
    if (!columnList) return;
    for (size_t index = 0; index < authorCount; index++) {
        int32_t node = authors[index];
        int32_t parent = backgroundParent(links, node);
        for (int depth = 0; depth < 8 && parent && parent != state->column; depth++) {
            node = parent;
            parent = backgroundParent(links, node);
        }
        if (parent != state->column) continue;
        state->header = node;
        rememberColumnChildren(bytes, columnList, state);
        rememberTimestamp(bytes, spans, count, links, state);
        return;
    }
}

static void keepQuotesInside(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    for (size_t slot = 0; slot < kInsideCapacity; slot++) {
        if (!gInsideRows[slot]) continue;
        BackgroundState *state = backgroundState(gInsideRows[slot], 0);
        int quoting = state && state->replyShown == AstralReplyInside;
        if (!state || (!quoting && !state->nameShown && state->timeShown == AstralTimeHeader)) {
            gInsideRows[slot] = 0;
            continue;
        }
        const Span *barList = quoting ? wireChildList(bytes, spans, count, state->replyBar) : NULL;
        const Span *textList = wireChildList(bytes, spans, count, state->quotedIn);
        const Span *columnList = state->nameShown ? wireChildList(bytes, spans, count, state->column) : NULL;
        const Span *timeList = state->timeShown != AstralTimeHeader ? wireChildList(bytes, spans, count, state->timeParent) : NULL;
        if (timeList) {
            rememberTimeParentChildren(bytes, timeList, state);
            putOriginalOnce(out, bytes, length, changed);
            writeTimeParentLayout(out, state);
        }
        if (!barList && !textList && !columnList) continue;
        if (barList) {
            state->barChildCount = 0;
            for (uint32_t position = 0; position < barList->childCount && state->barChildCount < kBubbleChildren; position++)
                state->barChildren[state->barChildCount++] = wireChildAt(bytes, barList, position);
            if (state->barChildCount > 1) state->reply = state->barChildren[1];
        }
        if (columnList) rememberColumnChildren(bytes, columnList, state);
        BubbleBlock *text = (BubbleBlock *)findBlock(state, state->quotedIn);
        if (!text) continue;
        if (textList) rememberBlockChildren(bytes, spans, count, text);
        putOriginalOnce(out, bytes, length, changed);
        if (quoting) {
            writeQuoteMove(out, state, text, 1, state->appliedRadius);
            if (barList) writePreviewStyle(out, state->reply, AstralReplyInside, state->appliedColor);
        }
        if (columnList) writeColumnLayout(out, state);
        if (!quoting) writeHostLayout(out, state, text);
    }
}

static BackgroundState *stateForColumn(int32_t column)
{
    if (!column) return NULL;
    for (size_t index = 0; index < kMaxDecorated; index++) {
        if (gBackgroundStates[index].column == column) return &gBackgroundStates[index];
    }
    return NULL;
}

static void resetBubbleNodes(BackgroundState *state)
{
    state->blockCount = 0;
    state->shownBlocks = 0;
    state->column = 0;
    state->reply = 0;
    state->bubble = 0;
    state->replyShown = 0;
    state->replyBar = 0;
    state->quotedIn = 0;
    state->quoteBox = 0;
    state->header = 0;
    state->timestamp = 0;
    state->timeParent = 0;
    state->timeParentChildCount = 0;
    state->columnChildCount = 0;
    state->nameShown = 0;
    state->timeShown = 0;
    state->avatar = 0;
    state->avatarShifted = 0;
    state->corners = 0;
    trackInside(state->id, 0);
}

static void rememberBlock(const uint8_t *bytes, const Span *spans, size_t count, const BackgroundLink *links, BackgroundState *state,
                          int32_t id, int media)
{
    BubbleBlock *block = addBlock(state, id, media);
    if (!block) return;
    const Span *creation = media ? wireCreation(spans, count, id) : NULL;
    if (creation) quadProperty(bytes, creation, P(padding), block->padding);
    rememberBlockChildren(bytes, spans, count, block);
    int32_t column = backgroundParent(links, id);
    if (column) state->column = column;
}

static void replaceTextBlock(BackgroundState *state, int32_t id)
{
    for (uint32_t index = 0; index < state->blockCount; index++) {
        BubbleBlock *block = &state->blocks[index];
        if (block->media || block->id == id) continue;
        if (state->quotedIn == block->id) state->quotedIn = id;
        *block = (BubbleBlock){.id = id};
        if (state->shownBlocks > index) state->shownBlocks = index;
        return;
    }
}

static void bubbleLateBlocks(const uint8_t *bytes, const Span *spans, size_t count, const BackgroundLink *links, const int32_t *blocks,
                             size_t blockCount, int media, Out *out, int *changed, size_t length)
{
    for (size_t index = 0; index < blockCount; index++) {
        if (!blocks[index]) continue;
        BackgroundState *state = stateForColumn(backgroundParent(links, blocks[index]));
        if (!state) continue;
        if (!media) replaceTextBlock(state, blocks[index]);
        rememberBlock(bytes, spans, count, links, state, blocks[index], media);
        if (!state->bubble || state->shownBlocks == state->blockCount) continue;
        putOriginalOnce(out, bytes, length, changed);
        for (uint32_t block = state->shownBlocks; block < state->blockCount; block++)
            writeBlockUpdate(out, &state->blocks[block], 1, state->appliedColor, state->appliedRadius);
        state->shownBlocks = state->blockCount;
        const BubbleBlock *quoted = findBlock(state, state->quotedIn);
        if (state->replyShown == AstralReplyInside && quoted) writeQuoteMove(out, state, quoted, 1, state->appliedRadius);
    }
}

static void bubbleLateNodes(const uint8_t *bytes, const Span *spans, size_t count, const BackgroundLink *links, const int32_t *contents,
                            size_t contentCount, const int32_t *media, size_t mediaCount, Out *out, int *changed, size_t length)
{
    bubbleLateBlocks(bytes, spans, count, links, contents, contentCount, 0, out, changed, length);
    bubbleLateBlocks(bytes, spans, count, links, media, mediaCount, 1, out, changed, length);
}

static void applyBackgroundStyle(Out *out, BackgroundState *state, const uint8_t *bytes, size_t length, int *changed)
{
    AstralMessageBackgroundContext context = {state->message, state->user, state->channel};
    AstralMessageBackgroundStyle style = {0};
    int result = gMessageBackgroundDecorator ? gMessageBackgroundDecorator(&context, &style) : 0;
    if (result < 0) {
        wireMarkRefreshBusy();
        return;
    }
    int active = result > 0 && validBackgroundStyle(&style);
    int bubble = active && style.bubble && state->blockCount;
    int reply = bubble && state->reply ? style.reply : AstralReplyAbove;
    if (reply == AstralReplyInside && !textBlock(state)) reply = AstralReplyBubble;
    int name = bubble && style.nameInside && state->header && state->columnChildCount && textBlock(state);
    int time = bubble && canMoveTime(state) ? style.time : AstralTimeHeader;
    if (time == AstralTimeInline && !name) time = AstralTimeHeader;
    uint32_t color = active ? style.color : state->originalColor;
    double radius = active ? style.radius : state->originalRadius;
    if (active == state->active && bubble == state->bubble && reply == state->replyShown && name == state->nameShown &&
        time == state->timeShown && (!bubble || state->shownBlocks == state->blockCount) &&
        (!active || (color == state->appliedColor && radius == state->appliedRadius)))
        return;
    if (bytes)
        putOriginalOnce(out, bytes, length, changed);
    else
        *changed = 1;
    if (bubble || state->bubble) writeBubbleUpdate(out, state, bubble, color, radius);
    if (reply != state->replyShown) writeReplyStyle(out, state, reply, color, radius);
    if (name != state->nameShown || time != state->timeShown) {
        const BubbleBlock *text = name ? textBlock(state) : findBlock(state, state->quotedIn);
        writeHeaderLayout(out, state, text, name, time);
        if (name && text)
            state->quotedIn = text->id;
        else if (state->replyShown != AstralReplyInside)
            state->quotedIn = 0;
        trackInside(state->id, state->replyShown == AstralReplyInside || state->nameShown || state->timeShown != AstralTimeHeader);
    }
    if (bubble)
        writeBackgroundUpdate(out, state, state->originalColor, state->originalRadius);
    else
        writeBackgroundUpdate(out, state, color, radius);
    state->bubble = bubble;
    state->shownBlocks = bubble ? state->blockCount : 0;
    if (active) {
        state->appliedColor = color;
        state->appliedRadius = radius;
    }
    state->active = active;
}

int astral_wire_refresh_message_backgrounds(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    Out out = {0};
    wirePut8(&out, kWireVersion);
    uint64_t channel = astral_wire_current_channel();
    int any = 0;
    for (size_t index = 0; index < kMaxDecorated; index++) {
        BackgroundState *state = &gBackgroundStates[index];
        if (state->id <= 0 || !state->hasOriginal || (channel && state->channel && state->channel != channel)) continue;
        applyBackgroundStyle(&out, state, NULL, 0, &any);
    }
    if (!any || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

static void forgetRemovedRows(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 4) continue;
        for (uint32_t position = 0; position < span->childCount; position++) {
            BackgroundState *state = backgroundState(wireChildAt(bytes, span, position), 0);
            if (!state) continue;
            trackInside(state->id, 0);
            state->id = -1;
            state->hasOriginal = 0;
        }
    }
}

static BackgroundState *bubbledRow(int32_t id)
{
    BackgroundState *state = backgroundState(id, 0);
    return state && state->hasOriginal && state->bubble ? state : NULL;
}

static void groupRows(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 3 || span->childCount < 2) continue;
        BackgroundState *previous = NULL;
        for (uint32_t position = 0; position < span->childCount; position++) {
            BackgroundState *state = bubbledRow(wireChildAt(bytes, span, position));
            if (!state) {
                previous = NULL;
                continue;
            }
            BackgroundState *next = position + 1 < span->childCount ? bubbledRow(wireChildAt(bytes, span, position + 1)) : NULL;
            int start = !previous || state->header;
            int end = !next || next->header;
            int corners = kCornerTopRight | kCornerBottomRight | (start ? kCornerTopLeft : 0) | (end ? kCornerBottomLeft : 0);
            previous = state;
            if (corners == state->corners) continue;
            state->corners = corners;
            putOriginalOnce(out, bytes, length, changed);
            writeCorners(out, state, 1, state->appliedRadius);
        }
    }
}

static void wireMessageBackgrounds(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    static BackgroundLink links[kBackgroundLinkCapacity];
    static int32_t rowMap[kBackgroundRowMapCapacity];
    static BackgroundRow rows[kBackgroundRowCapacity];
    static int32_t contents[kBackgroundRowCapacity];
    static int32_t media[kMediaCapacity];
    static int32_t authors[kBackgroundRowCapacity];
    size_t authorCount = 0;
    size_t contentCount = 0;
    size_t mediaCount = 0;
    memset(links, 0, sizeof(links));
    memset(rowMap, 0, sizeof(rowMap));
    memset(rows, 0, sizeof(rows));
    size_t linkCount = 0;
    size_t rowCount = 0;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 3) {
            for (uint32_t position = 0; position < span->childCount && linkCount < kMaxChildren; position++) {
                backgroundPutLink(links, wireChildAt(bytes, span, position), span->id);
                linkCount++;
            }
        } else if (span->op == 1 && wireStringPropertyPrefix(bytes, span, P(accessibilityId), kContentPrefix) &&
                   contentCount < kBackgroundRowCapacity) {
            contents[contentCount++] = span->id;
        } else if (span->op == 1 && wireStringProperty(bytes, span, P(accessibilityId), "message-row") &&
                   rowCount < kBackgroundRowCapacity) {
            rows[rowCount].id = span->id;
            backgroundPutRow(rowMap, span->id);
            rowCount++;
        } else if (mediaCount < kMediaCapacity && isMediaBlock(bytes, span)) {
            media[mediaCount++] = span->id;
        }
    }
    for (size_t index = 0; index < count && rowCount < kBackgroundRowCapacity; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2 && span->op != 3) continue;
        BackgroundState *state = backgroundState(span->id, 0);
        if (!state || !state->hasOriginal || backgroundWasRemoved(bytes, spans, count, span->id)) continue;
        if (backgroundRowIndex(rows, rowCount, span->id) < rowCount) continue;
        rows[rowCount] = (BackgroundRow){state->id, state->message, state->user, 0};
        backgroundPutRow(rowMap, state->id);
        rowCount++;
    }
    if (!rowCount) {
        bubbleLateNodes(bytes, spans, count, links, contents, contentCount, media, mediaCount, out, changed, length);
        keepQuotesInside(bytes, spans, count, out, changed, length);
        groupRows(bytes, spans, count, out, changed, length);
        forgetRemovedRows(bytes, spans, count);
        return;
    }
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2) continue;
        Tracked *tracked = wireTrackSlot(span->id, 0);
        if (!tracked || tracked->kind != kTrackAuthor) continue;
        if (authorCount < kBackgroundRowCapacity) authors[authorCount++] = span->id;
        int32_t id = backgroundRowForNode(links, rowMap, span->id);
        size_t rowIndex = backgroundRowIndex(rows, rowCount, id);
        if (rowIndex == rowCount || (rows[rowIndex].message && !wireCreation(spans, count, id))) continue;
        rows[rowIndex].message = tracked->value;
        rows[rowIndex].user = authorOf(tracked->value);
    }
    for (size_t index = 0; index < contentCount; index++) {
        int32_t id = backgroundRowForNode(links, rowMap, contents[index]);
        size_t rowIndex = backgroundRowIndex(rows, rowCount, id);
        if (rowIndex == rowCount) continue;
        rows[rowIndex].content = contents[index];
        if (!rows[rowIndex].message) rows[rowIndex].message = contentMessage(bytes, spans, count, contents[index]);
        contents[index] = 0;
    }
    for (size_t index = 0; index < rowCount; index++) {
        BackgroundRow *row = &rows[index];
        const Span *creation = wireCreation(spans, count, row->id);
        BackgroundState *state = backgroundState(row->id, 0);
        if (!row->message && !state && !gMessageBackgroundDecorator) continue;
        if (!state) state = backgroundState(row->id, 1);
        if (!state) continue;
        if (creation) {
            captureOriginalBackground(bytes, creation, state);
            resetBubbleNodes(state);
            rememberReply(bytes, spans, count, state, row->id);
            state->active = 0;
            state->message = row->message;
            state->user = row->user;
            state->channel = astral_wire_current_channel();
        } else if (row->message) {
            state->message = row->message;
            state->user = row->user;
            state->channel = astral_wire_current_channel();
        }
        if (!state->hasOriginal) captureOriginalBackground(bytes, creation, state);
        if (row->content) {
            replaceTextBlock(state, row->content);
            rememberBlock(bytes, spans, count, links, state, row->content, 0);
        }
        for (size_t block = 0; block < mediaCount; block++) {
            if (!media[block] || backgroundRowForNode(links, rowMap, media[block]) != row->id) continue;
            rememberBlock(bytes, spans, count, links, state, media[block], 1);
            media[block] = 0;
        }
        if (!state->header) rememberHeader(bytes, spans, count, links, authors, authorCount, state);
        if (!state->avatar) rememberAvatar(bytes, spans, count, links, rowMap, state);
        applyBackgroundStyle(out, state, bytes, length, changed);
    }
    bubbleLateNodes(bytes, spans, count, links, contents, contentCount, media, mediaCount, out, changed, length);
    keepQuotesInside(bytes, spans, count, out, changed, length);
    groupRows(bytes, spans, count, out, changed, length);
    forgetRemovedRows(bytes, spans, count);
}

static int32_t tagId(void)
{
    int32_t id = gNextTagId++;
    if (gNextTagId >= kFirstTagId + 0x00f00000) gNextTagId = kFirstTagId;
    return id;
}

int32_t wireAllocateNode(void)
{
    return tagId();
}

uint64_t wireRowMessage(int32_t row)
{
    BackgroundState *state = backgroundState(row, 0);
    return state && state->hasOriginal ? state->message : 0;
}

enum { kIconCacheSize = 8, kMaxIconFileBytes = 64 * 1024 };

typedef struct {
    char path[512];
    char *uri;
} IconData;

static IconData gIconData[kIconCacheSize];
static size_t gNextIconData;

static char *encodedIcon(const char *path)
{
    FILE *file = fopen(path, "rb");
    if (!file) return NULL;
    unsigned char *bytes = malloc(kMaxIconFileBytes);
    size_t size = bytes ? fread(bytes, 1, kMaxIconFileBytes, file) : 0;
    int truncated = bytes && fgetc(file) != EOF;
    fclose(file);
    if (!size || truncated) {
        free(bytes);
        return NULL;
    }
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const char *prefix = strstr(path, ".webp") ? "data:image/webp;base64," : "data:image/png;base64,";
    size_t prefixLength = strlen(prefix);
    char *uri = malloc(prefixLength + (size + 2) / 3 * 4 + 1);
    if (!uri) {
        free(bytes);
        return NULL;
    }
    memcpy(uri, prefix, prefixLength);
    char *cursor = uri + prefixLength;
    for (size_t index = 0; index < size; index += 3) {
        uint32_t chunk = (uint32_t)bytes[index] << 16;
        if (index + 1 < size) chunk |= (uint32_t)bytes[index + 1] << 8;
        if (index + 2 < size) chunk |= bytes[index + 2];
        *cursor++ = alphabet[(chunk >> 18) & 63];
        *cursor++ = alphabet[(chunk >> 12) & 63];
        *cursor++ = index + 1 < size ? alphabet[(chunk >> 6) & 63] : '=';
        *cursor++ = index + 2 < size ? alphabet[chunk & 63] : '=';
    }
    *cursor = 0;
    free(bytes);
    return uri;
}

static const char *iconURI(const char *icon)
{
    static const char kFilePrefix[] = "file://";
    if (strncmp(icon, kFilePrefix, sizeof(kFilePrefix) - 1) != 0) return icon;
    const char *path = icon + sizeof(kFilePrefix) - 1;
    for (size_t index = 0; index < kIconCacheSize; index++) {
        if (gIconData[index].uri && strcmp(gIconData[index].path, path) == 0) return gIconData[index].uri;
    }
    char *uri = encodedIcon(path);
    if (!uri) return NULL;
    IconData *slot = &gIconData[gNextIconData++ % kIconCacheSize];
    free(slot->uri);
    snprintf(slot->path, sizeof(slot->path), "%s", path);
    slot->uri = uri;
    return uri;
}

static int32_t buildIconTag(Out *out, const AstralTag *tag, Decorated *slot)
{
    const char *uri = iconURI(tag->icon);
    if (!uri) return 0;
    int32_t icon = tagId();
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)icon);
    wirePut16(out, 5);
    wirePut(out, "image", 5);
    wirePropCount(out, tag->tint ? 7 : 6);
    wirePropString(out, P(uri), uri);
    wirePropNumber(out, P(width), kIconTagSize);
    wirePropNumber(out, P(height), kIconTagSize);
    wirePropString(out, P(scale), "fit");
    wirePropNumber(out, P(shrink), 0);
    if (tag->action_id[0])
        wirePropString(out, P(accessibilityLabel), tag->text);
    else
        wirePropBool(out, P(accessibilityHidden), 1);
    if (tag->tint) wirePropLong(out, P(tint), (int64_t)tag->tint);
    slot->tags[slot->count++] = icon;
    return icon;
}

int32_t wireBuildTag(Out *out, const AstralTag *tag, Decorated *slot)
{
    if (tag->shape == kAstralTagIcon) return buildIconTag(out, tag, slot);
    int originalPoster = tag->shape == kAstralTagOriginalPoster;
    int gradient = tag->gradientCount >= 2;
    int32_t pill = tagId();
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)pill);
    wirePut16(out, 4);
    wirePut(out, "view", 4);
    wirePropCount(out, tag->action_id[0] ? 9 : 6);
    wirePropString(out, P(align), "center");
    if (gradient)
        wirePropGradient(out, P(backgroundGradient), 90, tag->gradient, tag->gradientCount > 3 ? 3 : tag->gradientCount);
    else
        wirePropLong(out, P(background), (int64_t)tag->background);
    wirePropString(out, P(direction), "row");
    wirePropNumber(out, P(height), originalPoster ? 16 : 18);
    wirePropQuad(out, P(padding), originalPoster ? 5 : 4, 0, originalPoster ? 5 : 4, 0);
    wirePropNumber(out, P(radius), originalPoster ? 8 : 4);
    if (tag->action_id[0]) {
        wirePropString(out, P(accessibilityId), tag->action_id);
        wirePropString(out, P(accessibilityLabel), tag->text);
        wirePropEvents(out, P(on), "press");
    }
    int32_t label = tagId();
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)label);
    wirePut16(out, 4);
    wirePut(out, "text", 4);
    wirePut16(out, originalPoster ? 8 : 7);
    wirePropLong(out, P(color), (int64_t)tag->foreground);
    wirePropNumber(out, P(fontSize), originalPoster ? 10 : 12);
    wirePropInt(out, P(fontWeight), originalPoster ? 700 : 600);
    wirePropNumber(out, P(lineHeight), originalPoster ? 13 : 16);
    wirePropInt(out, P(maxLines), 1);
    wirePropString(out, P(text), tag->text);
    wirePropBool(out, P(notoFallback), 1);
    if (originalPoster) {
        wirePropString(out, P(textTransform), "uppercase");
    }
    wirePutChildren(out, pill, (int32_t[]){label}, 1);
    slot->tags[slot->count++] = pill;
    slot->tags[slot->count++] = label;
    return pill;
}

void wireDecorate(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gDecorator && !gMessageBackgroundDecorator && !wirePatchingActive()) return;
    propagateAvatars(bytes, spans, count);
    wireMessageBackgrounds(bytes, spans, count, out, changed, length);
    if (!gDecorator) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 3 || span->childCount == 0 || span->childCount > 64) continue;
        int32_t matched = 0;
        uint64_t message = 0;
        for (uint32_t position = 0; position < span->childCount; position++) {
            Tracked *child = wireTrackSlot(wireChildAt(bytes, span, position), 0);
            if (child && child->kind == kTrackAuthor) {
                matched = child->id;
                message = child->value;
                break;
            }
        }
        if (!matched) continue;
        AstralDecorationContext context = {"message.author:", message, authorOf(message), astral_wire_current_channel()};
        AstralTag tags[kMaxTags];
        memset(tags, 0, sizeof(tags));
        size_t tagCount = gDecorator(&context, tags, kMaxTags);
        gStats.authors++;
        if (context.user_id) gStats.with_user++;
        if (context.channel_id) gStats.with_channel++;
        if (tagCount) gStats.tagged++;
        Decorated *slot = decoratedSlot(span->id, tagCount > 0);
        if (!tagCount && (!slot || !slot->count)) continue;
        if (!*changed) wirePut(out, bytes, length);
        *changed = 1;
        removeTags(out, slot);
        if (!tagCount) continue;
        int32_t children[72];
        uint32_t used = 0;
        int32_t created[kMaxTags];
        for (size_t tag = 0; tag < tagCount && tag < kMaxTags; tag++) created[tag] = wireBuildTag(out, &tags[tag], slot);
        uint32_t insertAt = span->childCount;
        for (uint32_t position = 0; position < span->childCount; position++) {
            Tracked *child = wireTrackSlot(wireChildAt(bytes, span, position), 0);
            if (child && child->kind == kTrackTimestamp) {
                insertAt = position;
                break;
            }
        }
        for (uint32_t position = 0; position <= span->childCount && used < 68; position++) {
            if (position == insertAt) {
                for (size_t tag = 0; tag < tagCount && tag < kMaxTags; tag++)
                    if (created[tag]) children[used++] = created[tag];
            }
            if (position < span->childCount) children[used++] = wireChildAt(bytes, span, position);
        }
        wirePutChildren(out, span->id, children, used);
    }
}

void wireObserveRemovals(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 4) continue;
        for (uint32_t position = 0; position < span->childCount; position++) {
            int32_t id = wireChildAt(bytes, span, position);
            Tracked *tracked = wireTrackSlot(id, 0);
            if (tracked && tracked->kind == kTrackChannel && atomic_load_explicit(&gCurrentChannel, memory_order_relaxed) == tracked->value)
                atomic_store_explicit(&gCurrentChannel, 0, memory_order_relaxed);
            forget(id);
            for (size_t backgroundIndex = 0; backgroundIndex < kMaxDecorated; backgroundIndex++) {
                if (gBackgroundStates[backgroundIndex].id == id) {
                    gBackgroundStates[backgroundIndex].active = 0;
                    gBackgroundStates[backgroundIndex].hasOriginal = 0;
                }
            }
            Decorated *slot = decoratedSlot(id, 0);
            if (slot && slot->count) {
                if (!*changed) wirePut(out, bytes, length);
                *changed = 1;
                removeTags(out, slot);
            }
        }
    }
}

void wireObserve(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++) {
        if (spans[index].op == 1 || spans[index].op == 2) observeProperties(bytes, &spans[index]);
    }
}
