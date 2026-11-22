#include "PhoenixWireInternal.h"

#include <stdlib.h>

enum { kMaxEmojiUrl = 512, kMaxEmojiDepth = 6 };

static AstralEmojiResolver gEmojiResolver;
static const char kEmojiScheme[] = "emoji://";

void astral_wire_set_emoji_resolver(AstralEmojiResolver resolver)
{
    gEmojiResolver = resolver;
}

static int resolveEmoji(const uint8_t *text, uint32_t length, char *url)
{
    size_t scheme = sizeof(kEmojiScheme) - 1;
    if (length <= scheme || length - scheme >= 128 || memcmp(text, kEmojiScheme, scheme) != 0) return 0;
    char codepoints[128];
    memcpy(codepoints, text + scheme, length - scheme);
    codepoints[length - scheme] = 0;
    return gEmojiResolver(codepoints, url, kMaxEmojiUrl) > 0;
}

static int copyValue(Out *out, Cursor *cursor, uint8_t tag, uint16_t key, int depth, int *replaced);

static int copyTagged(Out *out, Cursor *cursor, uint16_t key, int depth, int *replaced)
{
    uint8_t tag = wireRead8(cursor);
    wirePut8(out, tag);
    return copyValue(out, cursor, tag, key, depth, replaced);
}

static int copyValue(Out *out, Cursor *cursor, uint8_t tag, uint16_t key, int depth, int *replaced)
{
    if (depth > kMaxEmojiDepth || cursor->failed) return 0;
    if (tag == 6) {
        uint32_t length = wireRead32(cursor);
        if (!wireNeed(cursor, length)) return 0;
        const uint8_t *text = cursor->bytes + cursor->offset;
        char url[kMaxEmojiUrl];
        if (key == P(uri) && resolveEmoji(text, length, url)) {
            size_t urlLength = strlen(url);
            wirePut32(out, (uint32_t)urlLength);
            wirePut(out, url, urlLength);
            *replaced = 1;
        } else {
            wirePut32(out, length);
            wirePut(out, text, length);
        }
        cursor->offset += length;
        return 1;
    }
    if (tag == 7) {
        uint32_t count = wireRead32(cursor);
        wirePut32(out, count);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) copyTagged(out, cursor, 0, depth + 1, replaced);
        return !cursor->failed;
    }
    if (tag == 8) {
        uint32_t count = wireRead32(cursor);
        wirePut32(out, count);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            uint16_t entry = wireRead16(cursor);
            wirePut16(out, entry);
            copyTagged(out, cursor, entry, depth + 1, replaced);
        }
        return !cursor->failed;
    }
    size_t start = cursor->offset;
    wireSkipValue(cursor, tag, depth);
    if (cursor->failed) return 0;
    wirePut(out, cursor->bytes + start, cursor->offset - start);
    return 1;
}

static int containsEmoji(const uint8_t *bytes, const Span *span)
{
    size_t scheme = sizeof(kEmojiScheme) - 1;
    for (size_t index = span->start; index + scheme <= span->end; index++) {
        if (bytes[index] == 'e' && memcmp(bytes + index, kEmojiScheme, scheme) == 0) return 1;
    }
    return 0;
}

static int copyNode(const uint8_t *bytes, const Span *span, Out *out)
{
    Cursor cursor = {bytes, span->end, span->start, 0};
    size_t header = 1 + 4;
    if (span->op == 1) {
        cursor.offset += header;
        uint16_t typeLength = wireRead16(&cursor);
        wireSkip(&cursor, typeLength);
        header = cursor.offset - span->start;
    }
    wirePut(out, bytes + span->start, header);
    cursor.offset = span->start + header;
    uint16_t count = wireRead16(&cursor);
    wirePut16(out, count);
    int replaced = 0;
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        wirePut16(out, property);
        wirePut8(out, tag);
        if (property == P(uri) || property == P(spans)) {
            copyValue(out, &cursor, tag, property, 0, &replaced);
        } else {
            size_t at = cursor.offset;
            wireSkipValue(&cursor, tag, 0);
            if (!cursor.failed) wirePut(out, bytes + at, cursor.offset - at);
        }
    }
    if (!cursor.failed && cursor.offset < span->end) wirePut(out, bytes + cursor.offset, span->end - cursor.offset);
    return !cursor.failed && replaced;
}

uint8_t *wireEmojiBatch(const uint8_t *bytes, size_t length, Span *spans, size_t *count, size_t *newLength)
{
    if (!gEmojiResolver) return NULL;
    int any = 0;
    for (size_t index = 0; index < *count && !any; index++) {
        if ((spans[index].op == 1 || spans[index].op == 2) && containsEmoji(bytes, &spans[index])) any = 1;
    }
    if (!any) return NULL;
    Out out = {0};
    size_t cursor = 0;
    int replaced = 0;
    for (size_t index = 0; index < *count; index++) {
        const Span *span = &spans[index];
        if ((span->op != 1 && span->op != 2) || !containsEmoji(bytes, span)) continue;
        wirePut(&out, bytes + cursor, span->start - cursor);
        Out node = {0};
        if (copyNode(bytes, span, &node) && !node.failed) {
            wirePut(&out, node.bytes, node.length);
            replaced = 1;
        } else {
            wirePut(&out, bytes + span->start, span->end - span->start);
        }
        free(node.bytes);
        cursor = span->end;
    }
    wirePut(&out, bytes + cursor, length - cursor);
    size_t checked = 0;
    if (!replaced || out.failed || wireScan(out.bytes, out.length, spans, &checked) != 0) {
        free(out.bytes);
        if (replaced) wireScan(bytes, length, spans, count);
        return NULL;
    }
    *count = checked;
    *newLength = out.length;
    return out.bytes;
}

static const char kReactionPrefix[] = "message.reaction:";
static const char kUnicodeMarker[] = ":unicode:";

static int reactionCodepoints(const uint8_t *bytes, const Span *span, char *codepoints, size_t capacity)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property != P(accessibilityId) || tag != 6) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        uint32_t length = wireRead32(&cursor);
        if (!wireNeed(&cursor, length)) return 0;
        const char *text = (const char *)bytes + cursor.offset;
        size_t prefix = sizeof(kReactionPrefix) - 1, marker = sizeof(kUnicodeMarker) - 1;
        if (length <= prefix || memcmp(text, kReactionPrefix, prefix) != 0) return 0;
        for (uint32_t at = (uint32_t)prefix; at + marker < length; at++) {
            if (memcmp(text + at, kUnicodeMarker, marker) != 0) continue;
            size_t pointsLength = length - at - marker;
            if (pointsLength == 0 || pointsLength >= capacity) return 0;
            memcpy(codepoints, text + at + marker, pointsLength);
            codepoints[pointsLength] = 0;
            return 1;
        }
        return 0;
    }
    return 0;
}

static int32_t glyphText(const uint8_t *bytes, const Span *spans, size_t count, int32_t id, int depth)
{
    const Span *list = depth < 3 ? wireChildList(bytes, spans, count, id) : NULL;
    if (!list || list->childCount == 0) return 0;
    int32_t first = wireChildAt(bytes, list, 0);
    const Span *creation = wireCreation(spans, count, first);
    if (creation && wireTypeIs(bytes, creation, "text")) return first;
    return glyphText(bytes, spans, count, first, depth + 1);
}

static void putReactionImage(Out *out, int32_t text, const char *glyph, const char *url)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)text);
    wirePut16(out, 1);
    wirePut16(out, P(spans));
    wirePut8(out, 7);
    wirePut32(out, 1);
    wirePut8(out, 8);
    wirePut32(out, 2);
    wirePropString(out, P(text), glyph);
    wirePut16(out, P(image));
    wirePut8(out, 8);
    wirePut32(out, 2);
    wirePropString(out, P(uri), url);
    wirePropNumber(out, P(size), 20);
}

void wireGlyphOf(const char *codepoints, char *glyph, size_t capacity)
{
    size_t used = 0;
    const char *cursor = codepoints;
    while (*cursor && used + 4 < capacity) {
        unsigned long point = strtoul(cursor, (char **)&cursor, 16);
        if (point < 0x80) {
            glyph[used++] = (char)point;
        } else if (point < 0x800) {
            glyph[used++] = (char)(0xC0 | (point >> 6));
            glyph[used++] = (char)(0x80 | (point & 0x3F));
        } else if (point < 0x10000) {
            glyph[used++] = (char)(0xE0 | (point >> 12));
            glyph[used++] = (char)(0x80 | ((point >> 6) & 0x3F));
            glyph[used++] = (char)(0x80 | (point & 0x3F));
        } else {
            glyph[used++] = (char)(0xF0 | (point >> 18));
            glyph[used++] = (char)(0x80 | ((point >> 12) & 0x3F));
            glyph[used++] = (char)(0x80 | ((point >> 6) & 0x3F));
            glyph[used++] = (char)(0x80 | (point & 0x3F));
        }
        if (*cursor == '-') cursor++;
    }
    glyph[used] = 0;
}

void wireReactionEmoji(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gEmojiResolver) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        char codepoints[128], url[kMaxEmojiUrl], glyph[64];
        if (span->op != 1 || !reactionCodepoints(bytes, span, codepoints, sizeof(codepoints))) continue;
        int32_t text = glyphText(bytes, spans, count, span->id, 0);
        if (!text || gEmojiResolver(codepoints, url, sizeof(url)) <= 0) continue;
        wireGlyphOf(codepoints, glyph, sizeof(glyph));
        if (!*changed) wirePut(out, bytes, length);
        *changed = 1;
        putReactionImage(out, text, glyph, url);
    }
}
