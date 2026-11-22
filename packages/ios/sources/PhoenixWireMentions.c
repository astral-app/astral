#include "PhoenixWireInternal.h"

#include <stdio.h>
#include <stdlib.h>

enum { kMaxMentionSpans = 64, kMaxMentionText = 256, kMaxPending = 64, kMaxRetries = 30 };

typedef struct {
    int32_t id;
    uint64_t message;
    uint8_t *creation;
    size_t length;
    int tries;
} PendingMention;

static PendingMention gPending[kMaxPending];

static AstralMentionDecorator gMentionDecorator;
static AstralMessageTextColorizer gTextColorizer;
static const char kChipTint[] = "_chip_";

void astral_wire_set_message_text_colorizer(AstralMessageTextColorizer colorizer)
{
    gTextColorizer = colorizer;
}
static const char kContentPrefix[] = "message.content:";
static const char kAvatarAlt[] = "avatar";

void astral_wire_set_mention_decorator(AstralMentionDecorator decorator)
{
    gMentionDecorator = decorator;
}

typedef struct {
    size_t start;
    size_t end;
    size_t textAt;
    uint32_t textLength;
    int chip;
    int chipPart;
    uint32_t color;
} SpanItem;

static int contentMessage(const uint8_t *bytes, const Span *spans, size_t count, int32_t id, uint64_t *message)
{
    int32_t parent = wireParentOf(bytes, spans, count, id);
    const Span *creation = parent ? wireCreation(spans, count, parent) : NULL;
    if (!creation) return 0;
    Cursor cursor = {bytes, creation->end, creation->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t properties = wireRead16(&cursor);
    for (uint16_t index = 0; index < properties && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == P(accessibilityId) && tag == 6) {
            uint32_t length = wireRead32(&cursor);
            if (!wireNeed(&cursor, length)) return 0;
            return wirePrefixed(bytes + cursor.offset, length, kContentPrefix, message);
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static size_t readSpanItems(const uint8_t *bytes, Cursor *cursor, SpanItem *items)
{
    uint32_t total = wireRead32(cursor);
    size_t found = 0;
    for (uint32_t index = 0; index < total && !cursor->failed && found < kMaxMentionSpans; index++) {
        SpanItem item = {cursor->offset, 0, 0, 0, 0, 0, 0};
        uint8_t tag = wireRead8(cursor);
        if (tag != 8) return 0;
        uint32_t entries = wireRead32(cursor);
        int hasBackground = 0, hasLink = 0;
        for (uint32_t entry = 0; entry < entries && !cursor->failed; entry++) {
            uint16_t key = wireRead16(cursor);
            uint8_t valueTag = wireRead8(cursor);
            if (key == P(text) && valueTag == 6) {
                item.textLength = wireRead32(cursor);
                item.textAt = cursor->offset;
                wireSkip(cursor, item.textLength);
                continue;
            }
            if (key == P(bg)) hasBackground = 1;
            if (key == P(link)) hasLink = 1;
            if (key == P(color) && (valueTag == 3 || valueTag == 4) && wireNeed(cursor, 4)) memcpy(&item.color, bytes + cursor->offset, 4);
            wireSkipValue(cursor, valueTag, 1);
        }
        item.end = cursor->offset;
        item.chipPart = hasBackground && hasLink;
        item.chip = item.chipPart && item.textLength > 1 && bytes[item.textAt] == '@';
        items[found++] = item;
    }
    return cursor->failed || found != total ? 0 : found;
}

static uint32_t blended(uint32_t base, uint32_t tint)
{
    uint32_t amount = tint >> 24;
    if (amount == 0xFF) return tint;
    uint32_t result = base & 0xFF000000u;
    for (int shift = 0; shift <= 16; shift += 8) {
        uint32_t from = (base >> shift) & 0xFF, to = (tint >> shift) & 0xFF;
        result |= ((from * (255 - amount) + to * amount + 127) / 255) << shift;
    }
    return result;
}

typedef struct {
    int dropAt;
    uint32_t color;
    uint32_t background;
} SpanChange;

static void putSpan(Out *out, const uint8_t *bytes, const SpanItem *item, SpanChange change)
{
    if (!change.color && !change.background && !change.dropAt) {
        wirePut(out, bytes + item->start, item->end - item->start);
        return;
    }
    Cursor cursor = {bytes, item->end, item->start + 1, 0};
    uint32_t entries = wireRead32(&cursor);
    size_t kept[24][2];
    uint32_t keptCount = 0;
    for (uint32_t entry = 0; entry < entries && !cursor.failed && keptCount < 24; entry++) {
        size_t at = cursor.offset;
        uint16_t key = wireRead16(&cursor);
        uint8_t valueTag = wireRead8(&cursor);
        wireSkipValue(&cursor, valueTag, 1);
        if ((change.color && key == P(color)) || (change.background && key == P(bg))) continue;
        if (change.dropAt && key == P(text)) continue;
        kept[keptCount][0] = at;
        kept[keptCount][1] = cursor.offset;
        keptCount++;
    }
    uint32_t added = (change.color ? 1 : 0) + (change.background ? 1 : 0) + (change.dropAt ? 1 : 0);
    wirePut8(out, 8);
    wirePut32(out, keptCount + added);
    for (uint32_t index = 0; index < keptCount; index++) wirePut(out, bytes + kept[index][0], kept[index][1] - kept[index][0]);
    if (change.dropAt) {
        wirePut16(out, P(text));
        wirePut8(out, 6);
        wirePut32(out, item->textLength - 1);
        wirePut(out, bytes + item->textAt + 1, item->textLength - 1);
    }
    if (change.color) wirePropLong(out, P(color), (int64_t)change.color);
    if (change.background) wirePropInt(out, P(bg), (int32_t)change.background);
}

static void putAvatarSpan(Out *out, const uint8_t *bytes, const SpanItem *chip, const AstralMentionDecoration *decoration,
                          SpanChange change)
{
    Cursor cursor = {bytes, chip->end, chip->start + 1, 0};
    uint32_t entries = wireRead32(&cursor);
    size_t kept[16][2];
    uint32_t keptCount = 0;
    for (uint32_t entry = 0; entry < entries && !cursor.failed && keptCount < 16; entry++) {
        size_t at = cursor.offset;
        uint16_t key = wireRead16(&cursor);
        uint8_t valueTag = wireRead8(&cursor);
        wireSkipValue(&cursor, valueTag, 1);
        if ((change.background && key == P(bg)) || (change.color && key == P(color))) continue;
        if (key == P(bg) || key == P(bgRadius) || key == P(bgFitText) || key == P(link) || key == P(color)) {
            kept[keptCount][0] = at;
            kept[keptCount][1] = cursor.offset;
            keptCount++;
        }
    }
    wirePut8(out, 8);
    wirePut32(out, keptCount + 2 + (change.background ? 1 : 0) + (change.color ? 1 : 0));
    for (uint32_t index = 0; index < keptCount; index++) wirePut(out, bytes + kept[index][0], kept[index][1] - kept[index][0]);
    if (change.background) wirePropInt(out, P(bg), (int32_t)change.background);
    if (change.color) wirePropLong(out, P(color), (int64_t)change.color);
    wirePropString(out, P(text), kAvatarAlt);
    wirePut16(out, P(image));
    wirePut8(out, 8);
    wirePut32(out, decoration->round ? 4 : 3);
    char uri[sizeof(decoration->image)];
    const char *token = strstr(decoration->image, kChipTint);
    uint32_t chipColor = change.color ? change.color : chip->color;
    if (token && chipColor) {
        snprintf(uri, sizeof(uri), "%.*s_%06X_%s", (int)(token - decoration->image), decoration->image, chipColor & 0xFFFFFF,
                 token + sizeof(kChipTint) - 1);
    } else {
        snprintf(uri, sizeof(uri), "%s", decoration->image);
    }
    wirePropString(out, P(uri), uri);
    wirePropNumber(out, P(size), 16);
    wirePropString(out, P(pool), "small");
    if (decoration->round) wirePropNumber(out, P(radius), 8);
}

static int decorateText(const uint8_t *bytes, const Span *span, uint64_t message, Out *out, int *unresolved)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t properties = wireRead16(&cursor);
    for (uint16_t index = 0; index < properties && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property != P(spans) || tag != 7) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        SpanItem items[kMaxMentionSpans];
        size_t count = readSpanItems(bytes, &cursor, items);
        AstralMentionDecoration decorations[kMaxMentionSpans];
        int any = 0;
        uint32_t mention = 0;
        for (size_t item = 0; item < count; item++) {
            memset(&decorations[item], 0, sizeof(decorations[item]));
            if (!items[item].chip) continue;
            char text[kMaxMentionText];
            size_t length = items[item].textLength < sizeof(text) - 1 ? items[item].textLength : sizeof(text) - 1;
            memcpy(text, bytes + items[item].textAt, length);
            text[length] = 0;
            AstralMentionContext context = {message, astral_wire_current_channel(), text, mention++};
            if (gMentionDecorator && gMentionDecorator(&context, &decorations[item]) &&
                (decorations[item].image[0] || decorations[item].hide_at || decorations[item].color))
                any = 1;
            else if (unresolved)
                *unresolved = 1;
        }
        uint32_t textColor = 0;
        if (gTextColorizer && gTextColorizer(message, astral_wire_current_channel(), &textColor) < 0 && unresolved) *unresolved = 1;
        if (!any && !textColor) return 0;
        SpanChange changes[kMaxMentionSpans];
        memset(changes, 0, sizeof(changes));
        for (size_t item = 0; item < count; item++) {
            if (!items[item].chipPart && items[item].color && textColor) changes[item].color = blended(items[item].color, textColor);
            if (!items[item].chip) continue;
            changes[item].dropAt = decorations[item].hide_at;
            uint32_t chipColor = decorations[item].color;
            if (!chipColor) continue;
            uint32_t background = (chipColor & 0x00FFFFFF) | 0x26000000;
            for (size_t part = item ? item - 1 : 0; part <= item + 1 && part < count; part++) {
                if (!items[part].chipPart) continue;
                changes[part].color = chipColor | 0xFF000000;
                changes[part].background = background;
            }
        }
        uint32_t added = 0;
        for (size_t item = 0; item < count; item++) added += decorations[item].image[0] ? 1 : 0;
        wirePut8(out, 2);
        wirePut32(out, (uint32_t)span->id);
        wirePut16(out, 1);
        wirePut16(out, P(spans));
        wirePut8(out, 7);
        wirePut32(out, (uint32_t)count + added);
        for (size_t item = 0; item < count; item++) {
            int image = decorations[item].image[0] != 0;
            SpanChange chipChange = {0, changes[item].color, changes[item].background};
            if (image && !decorations[item].after) putAvatarSpan(out, bytes, &items[item], &decorations[item], chipChange);
            putSpan(out, bytes, &items[item], changes[item]);
            if (image && decorations[item].after) putAvatarSpan(out, bytes, &items[item], &decorations[item], chipChange);
        }
        return 1;
    }
    return 0;
}

static void dropPending(PendingMention *pending)
{
    free(pending->creation);
    memset(pending, 0, sizeof(*pending));
}

static void remember(const uint8_t *bytes, const Span *span, uint64_t message)
{
    PendingMention *free_slot = NULL;
    for (size_t index = 0; index < kMaxPending; index++) {
        if (gPending[index].id == span->id) return;
        if (!gPending[index].id && !free_slot) free_slot = &gPending[index];
    }
    if (!free_slot) return;
    free_slot->creation = malloc(span->end - span->start);
    if (!free_slot->creation) return;
    memcpy(free_slot->creation, bytes + span->start, span->end - span->start);
    free_slot->length = span->end - span->start;
    free_slot->id = span->id;
    free_slot->message = message;
    free_slot->tries = 0;
}

static int retryPending(Out *out)
{
    int sent = 0;
    for (size_t index = 0; index < kMaxPending; index++) {
        PendingMention *pending = &gPending[index];
        if (!pending->id) continue;
        Span span = {1, pending->id, 0, pending->length, 0, 0};
        int unresolved = 0;
        if (decorateText(pending->creation, &span, pending->message, out, &unresolved)) {
            sent = 1;
            dropPending(pending);
        } else if (++pending->tries >= kMaxRetries) {
            dropPending(pending);
        }
    }
    return sent;
}

int astral_wire_pending_mentions(void)
{
    int count = 0;
    for (size_t index = 0; index < kMaxPending; index++) count += gPending[index].id != 0;
    return count;
}

int astral_wire_retry_mentions(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    if ((!gMentionDecorator && !gTextColorizer) || !astral_wire_pending_mentions()) return 0;
    Out out = {0};
    wirePut8(&out, kWireVersion);
    if (!retryPending(&out) || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

void wireDecorateMentions(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gMentionDecorator && !gTextColorizer) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 4) {
            for (uint32_t position = 0; position < span->childCount; position++) {
                int32_t removed = wireChildAt(bytes, span, position);
                for (size_t slot = 0; slot < kMaxPending; slot++) {
                    if (gPending[slot].id == removed) dropPending(&gPending[slot]);
                }
            }
            continue;
        }
        uint64_t message = 0;
        if (span->op != 1 || !wireTypeIs(bytes, span, "text") || !contentMessage(bytes, spans, count, span->id, &message)) continue;
        Out update = {0};
        int unresolved = 0;
        if (decorateText(bytes, span, message, &update, &unresolved) && !update.failed) {
            if (!*changed) wirePut(out, bytes, length);
            *changed = 1;
            wirePut(out, update.bytes, update.length);
        } else if (unresolved) {
            remember(bytes, span, message);
        }
        free(update.bytes);
    }
}
