#include "PhoenixWireInternal.h"

#include <stdlib.h>

enum { kMaxNameText = 128 };

static AstralNameColorizer gNameColorizer;
static const char kHeaderRole[] = "header";
static const char kCountSeparator[] = " \xE2\x80\x94 ";

void astral_wire_set_name_colorizer(AstralNameColorizer colorizer)
{
    gNameColorizer = colorizer;
}

static int stringProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, const uint8_t **text, uint32_t *length)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 6) {
            *length = wireRead32(&cursor);
            if (!wireNeed(&cursor, *length)) return 0;
            *text = bytes + cursor.offset;
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static uint64_t avatarBefore(const uint8_t *bytes, const Span *spans, size_t count, int32_t parent, int32_t id)
{
    const Span *list = parent ? wireChildList(bytes, spans, count, parent) : NULL;
    if (!list) return 0;
    for (uint32_t position = 0; position < list->childCount; position++) {
        int32_t child = wireChildAt(bytes, list, position);
        if (child == id) return 0;
        const Span *creation = wireCreation(spans, count, child);
        const uint8_t *uri;
        uint32_t length;
        if (creation && wireTypeIs(bytes, creation, "image") && stringProperty(bytes, creation, P(uri), &uri, &length)) {
            uint64_t user = wireAvatarUser(uri, length);
            if (user) return user;
        }
    }
    return 0;
}

static uint64_t siblingAvatar(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    int32_t parent = wireParentOf(bytes, spans, count, id);
    uint64_t user = avatarBefore(bytes, spans, count, parent, id);
    if (user || !parent) return user;
    const Span *column = wireChildList(bytes, spans, count, parent);
    if (!column || column->childCount == 0 || wireChildAt(bytes, column, 0) != id) return 0;
    return avatarBefore(bytes, spans, count, wireParentOf(bytes, spans, count, parent), parent);
}

static const char kVoterPrefix[] = "voter-";

static uint64_t voterRow(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    int32_t parent = wireParentOf(bytes, spans, count, id);
    const Span *column = parent ? wireChildList(bytes, spans, count, parent) : NULL;
    if (!column || column->childCount == 0 || wireChildAt(bytes, column, 0) != id) return 0;
    int32_t ancestor = parent;
    for (int depth = 0; depth < 3 && ancestor; depth++) {
        ancestor = wireParentOf(bytes, spans, count, ancestor);
        const Span *creation = ancestor ? wireCreation(spans, count, ancestor) : NULL;
        const uint8_t *identifier;
        uint32_t length;
        uint64_t user = 0;
        if (creation && stringProperty(bytes, creation, P(accessibilityId), &identifier, &length) &&
            wirePrefixed(identifier, length, kVoterPrefix, &user))
            return user;
    }
    return 0;
}

static int isHeader(const uint8_t *bytes, const Span *span, const uint8_t *text, uint32_t length)
{
    const uint8_t *role;
    uint32_t roleLength;
    if (!stringProperty(bytes, span, P(role), &role, &roleLength) || roleLength != sizeof(kHeaderRole) - 1 ||
        memcmp(role, kHeaderRole, roleLength) != 0)
        return 0;
    size_t separator = sizeof(kCountSeparator) - 1;
    for (uint32_t index = 0; index + separator <= length; index++) {
        if (memcmp(text + index, kCountSeparator, separator) == 0) return 1;
    }
    return 0;
}

static double numberProperty(const uint8_t *bytes, const Span *span, uint16_t wanted)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 5 && wireNeed(&cursor, 8)) {
            double value;
            memcpy(&value, bytes + cursor.offset, 8);
            return value;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static int isTypingLine(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    int32_t parent = wireParentOf(bytes, spans, count, id);
    const Span *list = parent ? wireChildList(bytes, spans, count, parent) : NULL;
    if (!list) return 0;
    int dots = 0;
    for (uint32_t position = 0; position < list->childCount; position++) {
        int32_t child = wireChildAt(bytes, list, position);
        if (child == id) return dots >= 3;
        const Span *creation = wireCreation(spans, count, child);
        if (creation && wireTypeIs(bytes, creation, "view") && numberProperty(bytes, creation, P(width)) == 6 &&
            numberProperty(bytes, creation, P(height)) == 6 && numberProperty(bytes, creation, P(radius)) >= 999)
            dots++;
    }
    return 0;
}

typedef struct {
    size_t start;
    size_t end;
    size_t textAt;
    uint32_t textLength;
    int bold;
} NameSpan;

enum { kMaxNameSpans = 16 };

static int32_t gTypingNode;

static size_t readNameSpans(const uint8_t *bytes, const Span *span, NameSpan *items)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t properties = wireRead16(&cursor);
    for (uint16_t index = 0; index < properties && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property != P(spans) || tag != 7) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        uint32_t total = wireRead32(&cursor);
        if (total > kMaxNameSpans) return 0;
        for (uint32_t item = 0; item < total && !cursor.failed; item++) {
            NameSpan entry = {cursor.offset, 0, 0, 0, 0};
            if (wireRead8(&cursor) != 8) return 0;
            uint32_t entries = wireRead32(&cursor);
            for (uint32_t field = 0; field < entries && !cursor.failed; field++) {
                uint16_t key = wireRead16(&cursor);
                uint8_t valueTag = wireRead8(&cursor);
                if (key == P(text) && valueTag == 6) {
                    entry.textLength = wireRead32(&cursor);
                    entry.textAt = cursor.offset;
                    wireSkip(&cursor, entry.textLength);
                    continue;
                }
                if (key == P(weight) && valueTag == 3 && wireNeed(&cursor, 4)) {
                    int32_t weight;
                    memcpy(&weight, bytes + cursor.offset, 4);
                    entry.bold = weight >= 600;
                }
                wireSkipValue(&cursor, valueTag, 1);
            }
            entry.end = cursor.offset;
            items[item] = entry;
        }
        return cursor.failed ? 0 : total;
    }
    return 0;
}

static void putColoredSpan(Out *out, const uint8_t *bytes, const NameSpan *item, uint32_t color)
{
    if (!color) {
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
        if (key == P(color)) continue;
        kept[keptCount][0] = at;
        kept[keptCount][1] = cursor.offset;
        keptCount++;
    }
    wirePut8(out, 8);
    wirePut32(out, keptCount + 1);
    for (uint32_t index = 0; index < keptCount; index++) wirePut(out, bytes + kept[index][0], kept[index][1] - kept[index][0]);
    wirePropLong(out, P(color), (int64_t)color);
}

static int colorTyping(const uint8_t *bytes, const Span *span, Out *out)
{
    NameSpan items[kMaxNameSpans];
    uint32_t colors[kMaxNameSpans] = {0};
    size_t count = readNameSpans(bytes, span, items);
    int any = 0;
    for (size_t item = 0; item < count; item++) {
        if (!items[item].bold || !items[item].textLength || items[item].textLength >= kMaxNameText) continue;
        char name[kMaxNameText];
        memcpy(name, bytes + items[item].textAt, items[item].textLength);
        name[items[item].textLength] = 0;
        AstralNameContext context = {kAstralNameTyping, name, 0, astral_wire_current_channel()};
        if (gNameColorizer(&context, &colors[item]) && colors[item]) any = 1;
    }
    if (!any) return 0;
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)span->id);
    wirePut16(out, 1);
    wirePut16(out, P(spans));
    wirePut8(out, 7);
    wirePut32(out, (uint32_t)count);
    for (size_t item = 0; item < count; item++) putColoredSpan(out, bytes, &items[item], colors[item]);
    return 1;
}

static void putColor(Out *out, int32_t id, uint32_t color)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)id);
    wirePut16(out, 1);
    wirePropLong(out, P(color), (int64_t)color);
}

void wireColorNames(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gNameColorizer) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        int typing = span->op == 2 && gTypingNode && span->id == gTypingNode;
        if (!typing && span->op == 1 && wireTypeIs(bytes, span, "text") && isTypingLine(bytes, spans, count, span->id)) {
            gTypingNode = span->id;
            typing = 1;
        }
        if (typing) {
            Out update = {0};
            if (colorTyping(bytes, span, &update) && !update.failed) {
                if (!*changed) wirePut(out, bytes, length);
                *changed = 1;
                wirePut(out, update.bytes, update.length);
            }
            free(update.bytes);
            continue;
        }
        const uint8_t *text;
        uint32_t textLength;
        if (span->op != 1 || !wireTypeIs(bytes, span, "text") || !stringProperty(bytes, span, P(text), &text, &textLength) ||
            textLength == 0 || textLength >= kMaxNameText)
            continue;
        AstralNameContext context = {kAstralNameAvatar, NULL, 0, astral_wire_current_channel()};
        if (isHeader(bytes, span, text, textLength)) {
            context.kind = kAstralNameHeader;
        } else {
            context.user_id = siblingAvatar(bytes, spans, count, span->id);
            if (!context.user_id) {
                context.user_id = voterRow(bytes, spans, count, span->id);
                context.kind = kAstralNameVoter;
            }
            if (!context.user_id) continue;
        }
        char name[kMaxNameText];
        memcpy(name, text, textLength);
        name[textLength] = 0;
        context.text = name;
        uint32_t color = 0;
        if (!gNameColorizer(&context, &color) || !color) continue;
        if (!*changed) wirePut(out, bytes, length);
        *changed = 1;
        putColor(out, span->id, color);
    }
}
