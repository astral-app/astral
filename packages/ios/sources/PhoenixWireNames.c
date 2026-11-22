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
