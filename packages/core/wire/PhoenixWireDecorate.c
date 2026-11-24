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

enum { kBackgroundLinkCapacity = 16384, kBackgroundRowCapacity = 512, kBackgroundRowMapCapacity = 1024 };

typedef struct {
    int32_t id;
    int32_t parent;
} BackgroundLink;

typedef struct {
    int32_t id;
    uint64_t message;
    uint64_t user;
} BackgroundRow;

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

static void backgroundPutRow(int32_t *rows, int32_t id, size_t rowIndex)
{
    (void)rowIndex;
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

static void wireMessageBackgrounds(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    static BackgroundLink links[kBackgroundLinkCapacity];
    static int32_t rowMap[kBackgroundRowMapCapacity];
    static BackgroundRow rows[kBackgroundRowCapacity];
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
        } else if (span->op == 1 && wireStringProperty(bytes, span, P(accessibilityId), "message-row") &&
                   rowCount < kBackgroundRowCapacity) {
            rows[rowCount].id = span->id;
            backgroundPutRow(rowMap, span->id, rowCount);
            rowCount++;
        }
    }
    for (size_t index = 0; index < count && rowCount < kBackgroundRowCapacity; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2 && span->op != 3) continue;
        BackgroundState *state = backgroundState(span->id, 0);
        if (!state || !state->hasOriginal || backgroundWasRemoved(bytes, spans, count, span->id)) continue;
        if (backgroundRowIndex(rows, rowCount, span->id) < rowCount) continue;
        rows[rowCount] = (BackgroundRow){state->id, state->message, state->user};
        backgroundPutRow(rowMap, state->id, rowCount);
        rowCount++;
    }
    if (!rowCount) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2) continue;
        Tracked *tracked = wireTrackSlot(span->id, 0);
        if (!tracked || tracked->kind != kTrackAuthor) continue;
        int32_t id = backgroundRowForNode(links, rowMap, span->id);
        size_t rowIndex = backgroundRowIndex(rows, rowCount, id);
        if (rowIndex == rowCount || (rows[rowIndex].message && !wireCreation(spans, count, id))) continue;
        rows[rowIndex].message = tracked->value;
        rows[rowIndex].user = authorOf(tracked->value);
    }
    int wroteOriginal = 0;
    for (size_t index = 0; index < rowCount; index++) {
        BackgroundRow *row = &rows[index];
        const Span *creation = wireCreation(spans, count, row->id);
        BackgroundState *state = backgroundState(row->id, 0);
        if (!row->message && !state && !gMessageBackgroundDecorator) continue;
        if (!state) state = backgroundState(row->id, 1);
        if (!state) continue;
        if (creation) {
            captureOriginalBackground(bytes, creation, state);
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
        AstralMessageBackgroundContext context = {state->message, state->user, state->channel};
        AstralMessageBackgroundStyle style = {0};
        int result = gMessageBackgroundDecorator ? gMessageBackgroundDecorator(&context, &style) : 0;
        if (result < 0) continue;
        int active = result > 0 && validBackgroundStyle(&style);
        uint32_t color = active ? style.color : state->originalColor;
        double radius = active ? style.radius : state->originalRadius;
        if (active == state->active && (!active || (color == state->appliedColor && radius == state->appliedRadius))) continue;
        if (!wroteOriginal && !*changed) {
            wirePut(out, bytes, length);
            wroteOriginal = 1;
        }
        *changed = 1;
        writeBackgroundUpdate(out, state, color, radius);
        if (active) {
            state->appliedColor = color;
            state->appliedRadius = radius;
        }
        state->active = active;
    }
}

static int32_t tagId(void)
{
    int32_t id = gNextTagId++;
    if (gNextTagId >= kFirstTagId + 0x00f00000) gNextTagId = kFirstTagId;
    return id;
}

int32_t wireBuildTag(Out *out, const AstralTag *tag, Decorated *slot)
{
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
    if (!gDecorator && !gMessageBackgroundDecorator) return;
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
                for (size_t tag = 0; tag < tagCount && tag < kMaxTags; tag++) children[used++] = created[tag];
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
