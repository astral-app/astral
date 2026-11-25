#include "PhoenixWireInternal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { kMaxTrackedTexts = 512, kMaxOverride = 8192, kMaxNote = 64, kMaxTime = 48 };

static const char kContentPrefix[] = "message.content:";
static const uint32_t kNoteColor = 0x99FFFFFF;
static const double kTimeScale = 0.75;

typedef struct {
    uint64_t message;
    int32_t node;
    uint8_t *original;
    size_t originalLength;
    uint64_t applied;
    int overridden;
} TrackedText;

static TrackedText gTexts[kMaxTrackedTexts];
static AstralMessageTextProvider gTextProvider;
static AstralSpanTransformer gSpanTransformer;

void astral_wire_set_span_transformer(AstralSpanTransformer transformer)
{
    gSpanTransformer = transformer;
}

static uint64_t hashBytes(const uint8_t *bytes, size_t length)
{
    uint64_t hash = 1469598103934665603ull;
    for (size_t index = 0; index < length; index++) hash = (hash ^ bytes[index]) * 1099511628211ull;
    return hash ? hash : 1;
}

void astral_wire_set_message_text_provider(AstralMessageTextProvider provider)
{
    gTextProvider = provider;
}

static TrackedText *textFor(uint64_t message, int create)
{
    TrackedText *empty = NULL;
    for (size_t index = 0; index < kMaxTrackedTexts; index++) {
        if (gTexts[index].message == message) return &gTexts[index];
        if (!empty && !gTexts[index].message) empty = &gTexts[index];
    }
    if (!create) return NULL;
    if (!empty) {
        empty = &gTexts[message % kMaxTrackedTexts];
        free(empty->original);
    }
    memset(empty, 0, sizeof(*empty));
    empty->message = message;
    return empty;
}

static TrackedText *textForNode(int32_t node)
{
    for (size_t index = 0; index < kMaxTrackedTexts; index++)
        if (gTexts[index].message && gTexts[index].node == node) return &gTexts[index];
    return NULL;
}

static uint64_t contentMessageOf(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
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
            uint64_t message = 0;
            if (!wireNeed(&cursor, length)) return 0;
            return wirePrefixed(bytes + cursor.offset, length, kContentPrefix, &message) ? message : 0;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static int copySpans(const uint8_t *bytes, const Span *span, TrackedText *text)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t properties = wireRead16(&cursor);
    for (uint16_t index = 0; index < properties && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        size_t start = cursor.offset;
        uint8_t tag = wireRead8(&cursor);
        wireSkipValue(&cursor, tag, 0);
        if (property != P(spans) || cursor.failed) continue;
        free(text->original);
        text->originalLength = cursor.offset - start;
        text->original = malloc(text->originalLength);
        if (!text->original) {
            text->originalLength = 0;
            return 0;
        }
        memcpy(text->original, bytes + start, text->originalLength);
        return 1;
    }
    return 0;
}

static uint64_t hashOf(const char *text, const char *note)
{
    uint64_t hash = 1469598103934665603ull;
    for (const char *cursor = text; *cursor; cursor++) hash = (hash ^ (uint8_t)*cursor) * 1099511628211ull;
    hash = (hash ^ 0xff) * 1099511628211ull;
    for (const char *cursor = note; *cursor; cursor++) hash = (hash ^ (uint8_t)*cursor) * 1099511628211ull;
    return hash ? hash : 1;
}

static void putTimeSpan(Out *out, const char *time)
{
    char spaced[kMaxTime + 4];
    snprintf(spaced, sizeof(spaced), "   %s", time);
    wirePut8(out, 8);
    wirePut32(out, 3);
    wirePropString(out, P(text), spaced);
    wirePropLong(out, P(color), (int64_t)kNoteColor);
    wirePropNumber(out, P(scale), kTimeScale);
}

static void putOverride(Out *out, int32_t node, const char *text, const char *note, const char *time)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)node);
    wirePut16(out, 1);
    wirePut16(out, P(spans));
    wirePut8(out, 7);
    wirePut32(out, (note[0] ? 2 : 1) + (time[0] ? 1 : 0));
    wirePut8(out, 8);
    wirePut32(out, 1);
    wirePropString(out, P(text), text);
    if (!note[0]) {
        if (time[0]) putTimeSpan(out, time);
        return;
    }
    char suffix[kMaxNote + 4];
    size_t length = strlen(note);
    suffix[0] = ' ';
    suffix[1] = '(';
    memcpy(suffix + 2, note, length);
    suffix[length + 2] = ')';
    suffix[length + 3] = 0;
    wirePut8(out, 8);
    wirePut32(out, 2);
    wirePropString(out, P(text), suffix);
    wirePropLong(out, P(color), (int64_t)kNoteColor);
    if (time[0]) putTimeSpan(out, time);
}

static int putSpansWithTime(Out *out, int32_t node, const uint8_t *spans, size_t length, const char *time)
{
    if (!spans || length < 5 || spans[0] != 7) return 0;
    uint32_t count = 0;
    memcpy(&count, spans + 1, sizeof(count));
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)node);
    wirePut16(out, 1);
    wirePut16(out, P(spans));
    wirePut8(out, 7);
    wirePut32(out, count + 1);
    wirePut(out, spans + 5, length - 5);
    putTimeSpan(out, time);
    return 1;
}

static void putSpans(Out *out, int32_t node, const uint8_t *spans, size_t length)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)node);
    wirePut16(out, 1);
    wirePut16(out, P(spans));
    wirePut(out, spans, length);
}

static void startFade(Out *out, int32_t node)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)node);
    wirePropCount(out, 1);
    wirePropNumber(out, P(opacity), 0);
    wireTweenFade(node);
}

static int apply(Out *out, TrackedText *text, int force)
{
    static char override[kMaxOverride];
    static char note[kMaxNote];
    static char time[kMaxTime];
    override[0] = 0;
    note[0] = 0;
    time[0] = 0;
    int wanted = gTextProvider && gTextProvider(text->message, override, sizeof(override), note, sizeof(note)) && override[0];
    wireInlineTime(text->message, time, sizeof(time));
    uint8_t *transformed = NULL;
    size_t transformedLength = 0;
    if (!wanted && gSpanTransformer && text->original) {
        int result = gSpanTransformer(text->message, text->original, text->originalLength, &transformed, &transformedLength);
        if (result < 0) wireMarkRefreshBusy();
        if (result <= 0 || !transformed) {
            free(transformed);
            transformed = NULL;
        }
    }
    const uint8_t *base = transformed ? transformed : text->original;
    size_t baseLength = transformed ? transformedLength : text->originalLength;
    if (wanted || time[0] || transformed) {
        uint64_t hash = wanted ? hashOf(override, note) : transformed ? hashBytes(transformed, transformedLength) : 2;
        hash = (hash ^ hashOf(time, "")) | 1;
        if (!force && text->applied == hash) {
            free(transformed);
            return 0;
        }
        int overrideChanged = wanted != text->overridden;
        int wrote = 1;
        if (wanted)
            putOverride(out, text->node, override, note, time);
        else if (time[0])
            wrote = putSpansWithTime(out, text->node, base, baseLength, time);
        else
            putSpans(out, text->node, base, baseLength);
        free(transformed);
        if (!wrote) return 0;
        if (!force && overrideChanged) startFade(out, text->node);
        text->applied = hash;
        text->overridden = wanted;
        return 1;
    }
    if (!text->applied || !text->original) return 0;
    putSpans(out, text->node, text->original, text->originalLength);
    if (!force && text->overridden) startFade(out, text->node);
    text->applied = 0;
    text->overridden = 0;
    return 1;
}

void wireApplyMessageText(Out *out, uint64_t message)
{
    TrackedText *text = message ? textFor(message, 0) : NULL;
    if (text && text->node && text->original) apply(out, text, 0);
}

void wireMessageTexts(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    Out update = {0};
    int any = 0;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        TrackedText *text = NULL;
        if (span->op == 1 && wireTypeIs(bytes, span, "text")) {
            uint64_t message = contentMessageOf(bytes, spans, count, span->id);
            if (!message) continue;
            text = textFor(message, 1);
            text->node = span->id;
            text->applied = 0;
        } else if (span->op == 2) {
            text = textForNode(span->id);
            if (text) text->applied = 0;
        }
        if (!text || !copySpans(bytes, span, text)) continue;
        any |= apply(&update, text, 1);
    }
    if (any && !update.failed) {
        if (!*changed) wirePut(out, bytes, length);
        *changed = 1;
        wirePut(out, update.bytes, update.length);
    }
    free(update.bytes);
}

int astral_wire_refresh_message_texts(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    Out out = {0};
    wirePut8(&out, kWireVersion);
    int any = 0;
    for (size_t index = 0; index < kMaxTrackedTexts; index++) {
        if (gTexts[index].message && gTexts[index].node) any |= apply(&out, &gTexts[index], 0);
    }
    if (!any || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}
