#include "PhoenixWireInternal.h"

#include <stdlib.h>
#include <string.h>

enum { kEventHeader = 8, kMaxComposerText = 32768, kEventFrame = 1, kObjectTag = 8, kStringTag = 6, kIntTag = 3 };

static const char kSendButton[] = "send";
static const char kComposer[] = "composer";

typedef struct {
    int32_t node;
    uint16_t kind;
    uint16_t textKey;
    uint16_t countKey;
    int32_t count;
    size_t length;
    char text[kMaxComposerText];
} ComposerText;

static ComposerText gComposer;
static int32_t gSendNode;
static int32_t gComposerNode;
static char gComposerValue[kMaxComposerText];
static AstralSendTransform gSendTransform;

void astral_wire_set_send_transform(AstralSendTransform transform)
{
    gSendTransform = transform;
}

static void noteComposerValue(const uint8_t *bytes, const Span *span)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t properties = wireRead16(&cursor);
    for (uint16_t index = 0; index < properties && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property != P(value) || tag != kStringTag) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        uint32_t size = wireRead32(&cursor);
        if (!wireNeed(&cursor, size)) return;
        if (size >= kMaxComposerText) size = 0;
        memcpy(gComposerValue, bytes + cursor.offset, size);
        gComposerValue[size] = 0;
        return;
    }
}

void wireTrackComposer(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2) continue;
        if (wireStringProperty(bytes, span, P(accessibilityId), kSendButton)) gSendNode = span->id;
        if (span->op == 1 && wireStringProperty(bytes, span, P(accessibilityId), kComposer)) {
            gComposerNode = span->id;
            gComposerValue[0] = 0;
        }
        if (span->id == gComposerNode) noteComposerValue(bytes, span);
    }
}

static int32_t eventNode(const uint8_t *event)
{
    int32_t node;
    memcpy(&node, event + 2, sizeof(node));
    return node;
}

static uint16_t eventKind(const uint8_t *event)
{
    return (uint16_t)(event[6] | (event[7] << 8));
}

static int noteComposerText(const uint8_t *event, size_t length)
{
    Cursor cursor = {event, length, kEventHeader, 0};
    if (wireRead8(&cursor) != kObjectTag) return 0;
    uint32_t fields = wireRead32(&cursor);
    ComposerText next = {.node = eventNode(event), .kind = eventKind(event), .count = -1};
    int sawText = 0;
    for (uint32_t index = 0; index < fields && !cursor.failed; index++) {
        uint16_t key = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (tag == kStringTag && !sawText) {
            uint32_t size = wireRead32(&cursor);
            if (!wireNeed(&cursor, size) || size >= kMaxComposerText) return 0;
            next.textKey = key;
            next.length = size;
            memcpy(next.text, event + cursor.offset, size);
            next.text[size] = 0;
            wireSkip(&cursor, size);
            sawText = 1;
        } else if (tag == kIntTag) {
            next.countKey = key;
            next.count = (int32_t)wireRead32(&cursor);
        } else {
            wireSkipValue(&cursor, tag, 0);
        }
    }
    if (!sawText || next.count < 0 || cursor.failed) return 0;
    gComposer = next;
    return 1;
}

static void putChangeText(AstralWireBuffer *buffer, const char *text)
{
    Out out = {0};
    size_t length = strlen(text);
    wirePut8(&out, kEventFrame);
    wirePut8(&out, kEventFrame);
    wirePut32(&out, (uint32_t)gComposer.node);
    wirePut16(&out, gComposer.kind);
    wirePut8(&out, kObjectTag);
    wirePut32(&out, 2);
    wirePut16(&out, gComposer.textKey);
    wirePut8(&out, kStringTag);
    wirePut32(&out, (uint32_t)length);
    wirePut(&out, text, length);
    wirePut16(&out, gComposer.countKey);
    wirePut8(&out, kIntTag);
    wirePut32(&out, (uint32_t)(gComposer.count + 1));
    if (out.failed) {
        free(out.bytes);
        return;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
}

int astral_wire_composer_event(const uint8_t *event, size_t length, AstralWireBuffer *before)
{
    memset(before, 0, sizeof(*before));
    if (length < kEventHeader || event[0] != kEventFrame || event[1] != kEventFrame) return 0;
    uint16_t kind = eventKind(event);
    if (kind == P(changeText)) {
        if (eventNode(event) == gComposerNode) noteComposerText(event, length);
        return 0;
    }
    if (kind != P(press) || !gSendNode || eventNode(event) != gSendNode || !gSendTransform) return 0;
    if (!gComposerNode || gComposer.node != gComposerNode || strcmp(gComposer.text, gComposerValue) != 0) return 0;
    static char rewritten[kMaxComposerText];
    rewritten[0] = 0;
    if (!gSendTransform(astral_wire_current_channel(), gComposer.text, rewritten, sizeof(rewritten))) return 0;
    if (strcmp(rewritten, gComposer.text) == 0) return 0;
    putChangeText(before, rewritten);
    return before->bytes != NULL;
}

int32_t wireComposerNode(void)
{
    return gComposerNode;
}
