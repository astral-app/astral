#include "PhoenixWireInternal.h"

#include <stdlib.h>

enum { kMaxMentionSpans = 64, kMaxMentionText = 256 };

static AstralMentionDecorator gMentionDecorator;
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
        SpanItem item = {cursor->offset, 0, 0, 0, 0};
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
            wireSkipValue(cursor, valueTag, 1);
        }
        item.end = cursor->offset;
        item.chip = hasBackground && hasLink && item.textLength > 1 && bytes[item.textAt] == '@';
        items[found++] = item;
    }
    return cursor->failed || found != total ? 0 : found;
}

static void putSpanCopy(Out *out, const uint8_t *bytes, const SpanItem *item, int dropAt)
{
    if (!dropAt) {
        wirePut(out, bytes + item->start, item->end - item->start);
        return;
    }
    size_t textHeader = item->textAt - 4;
    wirePut(out, bytes + item->start, textHeader - item->start);
    wirePut32(out, item->textLength - 1);
    wirePut(out, bytes + item->textAt + 1, item->end - item->textAt - 1);
}

static void putAvatarSpan(Out *out, const uint8_t *bytes, const SpanItem *chip, const AstralMentionDecoration *decoration)
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
        if (key == P(bg) || key == P(bgRadius) || key == P(bgFitText) || key == P(link) || key == P(color)) {
            kept[keptCount][0] = at;
            kept[keptCount][1] = cursor.offset;
            keptCount++;
        }
    }
    wirePut8(out, 8);
    wirePut32(out, keptCount + 2);
    for (uint32_t index = 0; index < keptCount; index++) wirePut(out, bytes + kept[index][0], kept[index][1] - kept[index][0]);
    wirePropString(out, P(text), kAvatarAlt);
    wirePut16(out, P(image));
    wirePut8(out, 8);
    wirePut32(out, decoration->round ? 4 : 3);
    wirePropString(out, P(uri), decoration->image);
    wirePropNumber(out, P(size), 16);
    wirePropString(out, P(pool), "small");
    if (decoration->round) wirePropNumber(out, P(radius), 8);
}

static int decorateText(const uint8_t *bytes, const Span *span, uint64_t message, Out *out)
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
            if (gMentionDecorator(&context, &decorations[item]) && (decorations[item].image[0] || decorations[item].hide_at)) any = 1;
        }
        if (!any) return 0;
        uint32_t added = 0;
        for (size_t item = 0; item < count; item++) added += decorations[item].image[0] ? 1 : 0;
        wirePut8(out, 2);
        wirePut32(out, (uint32_t)span->id);
        wirePut16(out, 1);
        wirePut16(out, P(spans));
        wirePut8(out, 7);
        wirePut32(out, (uint32_t)count + added);
        for (size_t item = 0; item < count; item++) {
            if (decorations[item].image[0]) putAvatarSpan(out, bytes, &items[item], &decorations[item]);
            putSpanCopy(out, bytes, &items[item], decorations[item].hide_at);
        }
        return 1;
    }
    return 0;
}

void wireDecorateMentions(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gMentionDecorator) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        uint64_t message = 0;
        if (span->op != 1 || !wireTypeIs(bytes, span, "text") || !contentMessage(bytes, spans, count, span->id, &message)) continue;
        Out update = {0};
        if (decorateText(bytes, span, message, &update) && !update.failed) {
            if (!*changed) wirePut(out, bytes, length);
            *changed = 1;
            wirePut(out, update.bytes, update.length);
        }
        free(update.bytes);
    }
}
