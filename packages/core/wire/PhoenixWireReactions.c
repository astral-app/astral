#include "PhoenixWireInternal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { kMaxPills = 256, kMaxPillChildren = 8, kMaxStackNodes = kAstralReactionFaces * 2 + 4, kMaxRetries = 20 };

static const int32_t kFirstStackId = 0x7c000000;
static const char kReactionPrefix[] = "message.reaction:";
static const double kFaceSize = 16.2;
static const double kFaceStep = 4.5;
static const double kBadgeFontSize = 7.2;
static const double kTrailingInset = -2;

typedef struct {
    int32_t pill;
    uint64_t message;
    char emoji[96];
    int burst;
    int32_t children[kMaxPillChildren];
    uint32_t childCount;
    int32_t countText;
    uint32_t total;
    int32_t nodes[kMaxStackNodes];
    uint32_t nodeCount;
    uint64_t shown;
    uint8_t dirty;
    uint8_t pending;
    uint8_t tries;
} Pill;

static Pill gPills[kMaxPills];
static AstralReactionDecorator gReactionDecorator;
static int32_t gNextStackId = kFirstStackId;

void astral_wire_set_reaction_decorator(AstralReactionDecorator decorator)
{
    gReactionDecorator = decorator;
}

static int32_t stackId(void)
{
    int32_t id = gNextStackId++;
    if (gNextStackId >= kFirstStackId + 0x00f00000) gNextStackId = kFirstStackId;
    return id;
}

static Pill *pillWithId(int32_t id)
{
    for (size_t index = 0; index < kMaxPills; index++) {
        if (gPills[index].pill == id) return &gPills[index];
    }
    return NULL;
}

static Pill *pillWithCount(int32_t text)
{
    for (size_t index = 0; index < kMaxPills; index++) {
        if (gPills[index].pill && gPills[index].countText == text) return &gPills[index];
    }
    return NULL;
}

static Pill *freePill(void)
{
    for (size_t index = 0; index < kMaxPills; index++) {
        if (!gPills[index].pill) return &gPills[index];
    }
    return NULL;
}

static int ownsNode(const Pill *pill, int32_t id)
{
    for (uint32_t index = 0; index < pill->nodeCount; index++) {
        if (pill->nodes[index] == id) return 1;
    }
    return 0;
}

static int readString(const uint8_t *bytes, const Span *span, uint16_t wanted, const uint8_t **text, uint32_t *length)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
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

static uint32_t parseCount(const uint8_t *text, uint32_t length)
{
    uint32_t value = 0;
    for (uint32_t index = 0; index < length && text[index] >= '0' && text[index] <= '9'; index++) {
        value = value * 10 + (uint32_t)(text[index] - '0');
    }
    return value;
}

static const char *field(const char *text, const char *end, char *out, size_t capacity)
{
    size_t used = 0;
    while (text < end && *text != ':' && used + 1 < capacity) out[used++] = *text++;
    out[used] = 0;
    return text < end && *text == ':' ? text + 1 : end;
}

static int parseReaction(const uint8_t *bytes, const Span *span, Pill *pill)
{
    const uint8_t *text;
    uint32_t length;
    size_t prefix = sizeof(kReactionPrefix) - 1;
    if (!readString(bytes, span, P(accessibilityId), &text, &length)) return 0;
    if (length <= prefix || length >= 256 || memcmp(text, kReactionPrefix, prefix) != 0) return 0;
    const char *cursor = (const char *)text + prefix, *end = (const char *)text + length;
    char message[24], kind[16], type[16], value[128];
    cursor = field(cursor, end, message, sizeof(message));
    cursor = field(cursor, end, kind, sizeof(kind));
    cursor = field(cursor, end, type, sizeof(type));
    size_t valueLength = (size_t)(end - cursor) < sizeof(value) - 1 ? (size_t)(end - cursor) : sizeof(value) - 1;
    memcpy(value, cursor, valueLength);
    value[valueLength] = 0;
    pill->message = strtoull(message, NULL, 10);
    if (!pill->message || !valueLength) return 0;
    pill->burst = strcmp(kind, "burst") == 0;
    if (strcmp(type, "unicode") == 0) {
        wireGlyphOf(value, pill->emoji, sizeof(pill->emoji));
    } else {
        const char *id = strrchr(value, ':');
        snprintf(pill->emoji, sizeof(pill->emoji), "e:%s", id ? id + 1 : value);
    }
    return pill->emoji[0] != 0;
}

static int32_t firstText(const uint8_t *bytes, const Span *spans, size_t count, int32_t id, int depth)
{
    const Span *creation = wireCreation(spans, count, id);
    if (creation && wireTypeIs(bytes, creation, "text")) return id;
    const Span *list = depth < 3 ? wireChildList(bytes, spans, count, id) : NULL;
    for (uint32_t position = 0; list && position < list->childCount; position++) {
        int32_t found = firstText(bytes, spans, count, wireChildAt(bytes, list, position), depth + 1);
        if (found) return found;
    }
    return 0;
}

static void adoptChildren(Pill *pill, const uint8_t *bytes, const Span *list)
{
    pill->childCount = 0;
    for (uint32_t position = 0; position < list->childCount && pill->childCount < kMaxPillChildren; position++) {
        int32_t child = wireChildAt(bytes, list, position);
        if (!ownsNode(pill, child)) pill->children[pill->childCount++] = child;
    }
}

static void locateCount(Pill *pill, const uint8_t *bytes, const Span *spans, size_t count)
{
    if (pill->childCount < 2) return;
    int32_t text = firstText(bytes, spans, count, pill->children[pill->childCount - 1], 0);
    const Span *creation = text ? wireCreation(spans, count, text) : NULL;
    if (!creation) return;
    pill->countText = text;
    const uint8_t *value;
    uint32_t length;
    if (readString(bytes, creation, P(text), &value, &length)) pill->total = parseCount(value, length);
}

static void registerPill(const uint8_t *bytes, const Span *spans, size_t count, const Span *span)
{
    Pill parsed = {0};
    if (!parseReaction(bytes, span, &parsed)) return;
    Pill *pill = pillWithId(span->id);
    if (!pill) pill = freePill();
    if (!pill) return;
    *pill = parsed;
    pill->pill = span->id;
    const Span *list = wireChildList(bytes, spans, count, span->id);
    if (list) adoptChildren(pill, bytes, list);
    locateCount(pill, bytes, spans, count);
    pill->dirty = pill->childCount > 0 && pill->total > 0;
}

static uint64_t decorationHash(const AstralReactionDecoration *decoration)
{
    uint64_t hash = 1469598103934665603ull ^ decoration->total;
    for (uint32_t face = 0; face < decoration->image_count; face++) {
        for (const char *cursor = decoration->images[face]; *cursor; cursor++) hash = (hash ^ (uint8_t)*cursor) * 1099511628211ull;
        hash = (hash ^ 0xff) * 1099511628211ull;
    }
    return hash ? hash : 1;
}

static double stackWidth(uint32_t members)
{
    return members ? kFaceSize + (members - 1) * kFaceStep : 0;
}

static void removeStack(Out *out, Pill *pill)
{
    if (!pill->nodeCount) return;
    for (uint32_t index = 0; index < pill->nodeCount; index++) wireTweenForget(pill->nodes[index]);
    wirePut8(out, 4);
    wirePut32(out, pill->nodeCount);
    for (uint32_t index = 0; index < pill->nodeCount; index++) wirePut32(out, (uint32_t)pill->nodes[index]);
    pill->nodeCount = 0;
}

static int32_t newStackNode(Out *out, Pill *pill, const char *type)
{
    int32_t id = stackId();
    size_t length = strlen(type);
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)id);
    wirePut16(out, (uint16_t)length);
    wirePut(out, type, length);
    if (pill->nodeCount < kMaxStackNodes) pill->nodes[pill->nodeCount++] = id;
    return id;
}

static int32_t putFace(Out *out, Pill *pill, const char *image, int first)
{
    int32_t face = newStackNode(out, pill, "image");
    wirePropCount(out, 8);
    wirePropString(out, P(uri), image);
    wirePropString(out, P(pool), "small");
    wirePropNumber(out, P(width), kFaceSize);
    wirePropNumber(out, P(height), kFaceSize);
    wirePropNumber(out, P(radius), kFaceSize / 2);
    wirePropString(out, P(scale), "fill");
    wirePropQuad(out, P(margin), first ? 0 : (float)(kFaceStep - kFaceSize), 0, 0, 0);
    wirePropNumber(out, P(shrink), 0);
    return face;
}

static int32_t putBadge(Out *out, Pill *pill, uint32_t overflow)
{
    char label[16];
    snprintf(label, sizeof(label), "+%u", overflow);
    int32_t text = newStackNode(out, pill, "text");
    wirePropCount(out, 6);
    wirePropLong(out, P(color), (int64_t)0xFFFFFFFF);
    wirePropNumber(out, P(fontSize), kBadgeFontSize);
    wirePropInt(out, P(fontWeight), 600);
    wirePropInt(out, P(maxLines), 1);
    wirePropString(out, P(text), label);
    wirePropBool(out, P(notoFallback), 1);
    int32_t badge = newStackNode(out, pill, "view");
    wirePropCount(out, 8);
    wirePropLong(out, P(background), (int64_t)0xFF000000);
    wirePropNumber(out, P(width), kFaceSize);
    wirePropNumber(out, P(height), kFaceSize);
    wirePropNumber(out, P(radius), kFaceSize / 2);
    wirePropString(out, P(align), "center");
    wirePropString(out, P(justify), "center");
    wirePropQuad(out, P(margin), (float)(kFaceStep - kFaceSize), 0, 0, 0);
    wirePropNumber(out, P(shrink), 0);
    wirePutChildren(out, badge, (int32_t[]){text}, 1);
    return badge;
}

static void putStack(Out *out, Pill *pill, const AstralReactionDecoration *decoration)
{
    int32_t members[kAstralReactionFaces + 1];
    uint32_t used = 0;
    uint32_t faces = decoration->image_count > kAstralReactionFaces ? kAstralReactionFaces : decoration->image_count;
    for (uint32_t face = 0; face < faces; face++) members[used++] = putFace(out, pill, decoration->images[face], face == 0);
    if (decoration->total > faces && faces == kAstralReactionFaces) members[used++] = putBadge(out, pill, decoration->total - faces);
    int32_t stack = newStackNode(out, pill, "view");
    wirePropCount(out, 7);
    wirePropString(out, P(direction), "row");
    wirePropString(out, P(align), "center");
    wirePropQuad(out, P(margin), 0, 0, 0, 0);
    wirePropNumber(out, P(shrink), 0);
    wirePropBool(out, P(clip), 1);
    wirePropNumber(out, P(opacity), 0);
    wirePropNumber(out, P(maxWidth), 0);
    wireTweenIn(stack, stackWidth(used), kTrailingInset);
    wirePutChildren(out, stack, members, used);
    int32_t children[kMaxPillChildren + 1];
    memcpy(children, pill->children, pill->childCount * sizeof(int32_t));
    children[pill->childCount] = stack;
    wirePutChildren(out, pill->pill, children, pill->childCount + 1);
}

static void restorePill(Out *out, Pill *pill)
{
    removeStack(out, pill);
    wirePutChildren(out, pill->pill, pill->children, pill->childCount);
    pill->shown = 0;
}

static int refreshPill(Out *out, Pill *pill, int force)
{
    if (!pill->total) {
        if (!pill->nodeCount) return 0;
        restorePill(out, pill);
        return 1;
    }
    AstralReactionContext context = {pill->message, astral_wire_current_channel(), pill->emoji, pill->burst, pill->total};
    AstralReactionDecoration decoration;
    memset(&decoration, 0, sizeof(decoration));
    if (!gReactionDecorator(&context, &decoration) || !decoration.image_count) {
        pill->pending = 1;
        if (!force || !pill->nodeCount) return 0;
        removeStack(out, pill);
        pill->shown = 0;
        return 1;
    }
    pill->pending = decoration.stale != 0;
    if (!pill->pending) pill->tries = 0;
    if (!decoration.total) decoration.total = pill->total;
    uint64_t hash = decorationHash(&decoration);
    if (!force && hash == pill->shown && pill->nodeCount) return 0;
    removeStack(out, pill);
    putStack(out, pill, &decoration);
    pill->shown = hash;
    return 1;
}

static void forgetRemoved(const uint8_t *bytes, const Span *span)
{
    for (uint32_t position = 0; position < span->childCount; position++) {
        Pill *pill = pillWithId(wireChildAt(bytes, span, position));
        if (!pill) continue;
        for (uint32_t index = 0; index < pill->nodeCount; index++) wireTweenForget(pill->nodes[index]);
        memset(pill, 0, sizeof(*pill));
    }
}

static void noteUpdate(const uint8_t *bytes, const Span *span)
{
    Pill *pill = pillWithCount(span->id);
    const uint8_t *value;
    uint32_t length;
    if (!pill || !readString(bytes, span, P(text), &value, &length)) return;
    uint32_t total = parseCount(value, length);
    if (total == pill->total) return;
    pill->total = total;
    pill->dirty = 1;
}

static void noteChildren(const uint8_t *bytes, const Span *spans, size_t count, const Span *span, uint8_t *forced)
{
    Pill *pill = pillWithId(span->id);
    if (!pill) return;
    adoptChildren(pill, bytes, span);
    locateCount(pill, bytes, spans, count);
    if (pill->nodeCount) *forced = 1;
    pill->dirty = 1;
}

void wireReactionAvatars(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gReactionDecorator) return;
    uint8_t forced[kMaxPills] = {0};
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 4)
            forgetRemoved(bytes, span);
        else if (span->op == 1 && wireTypeIs(bytes, span, "view"))
            registerPill(bytes, spans, count, span);
        else if (span->op == 2)
            noteUpdate(bytes, span);
    }
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 3 || wireCreation(spans, count, span->id)) continue;
        Pill *pill = pillWithId(span->id);
        if (pill) noteChildren(bytes, spans, count, span, &forced[pill - gPills]);
    }
    for (size_t index = 0; index < kMaxPills; index++) {
        Pill *pill = &gPills[index];
        if (!pill->pill || !pill->dirty) continue;
        pill->dirty = 0;
        Out update = {0};
        if (refreshPill(&update, pill, forced[index]) && !update.failed) {
            if (!*changed) wirePut(out, bytes, length);
            *changed = 1;
            wirePut(out, update.bytes, update.length);
        }
        free(update.bytes);
    }
}

int astral_wire_refresh_reactions(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    Out out = {0};
    wirePut8(&out, kWireVersion);
    int sent = 0;
    for (size_t index = 0; index < kMaxPills; index++) {
        Pill *pill = &gPills[index];
        if (!pill->pill || !pill->childCount) continue;
        pill->tries = 0;
        if (gReactionDecorator) sent |= refreshPill(&out, pill, 0);
        if (pill->nodeCount && (!gReactionDecorator || pill->pending)) {
            restorePill(&out, pill);
            sent = 1;
        }
    }
    if (!sent || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

int astral_wire_pending_reactions(void)
{
    if (!gReactionDecorator) return 0;
    int pending = 0;
    for (size_t index = 0; index < kMaxPills; index++) pending += gPills[index].pill && gPills[index].pending;
    return pending;
}

int astral_wire_retry_reactions(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    if (!astral_wire_pending_reactions()) return 0;
    Out out = {0};
    wirePut8(&out, kWireVersion);
    int sent = 0;
    for (size_t index = 0; index < kMaxPills; index++) {
        Pill *pill = &gPills[index];
        if (!pill->pill || !pill->pending) continue;
        if (++pill->tries > kMaxRetries) {
            pill->pending = 0;
            continue;
        }
        sent |= refreshPill(&out, pill, 0);
    }
    if (!sent || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}
