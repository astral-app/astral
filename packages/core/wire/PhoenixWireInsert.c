#include "PhoenixWireInternal.h"

#include <stdlib.h>
#include <string.h>

enum { kInsertPrefixCapacity = 32, kInsertPrefixLength = 96, kInsertTargetCapacity = 256, kTargetChildren = 48 };

typedef struct {
    int32_t id;
    char identifier[kWireIdentifierLength];
    char type[kWireTypeLength];
    int32_t children[kTargetChildren];
    uint32_t childCount;
    int32_t inserted[kAstralInsertCapacity];
    uint32_t insertedCount;
    uint32_t topLevel;
    int atStart;
} InsertTarget;

static AstralNodeInserter gInserter;
static char gPrefixes[kInsertPrefixCapacity][kInsertPrefixLength];
static size_t gPrefixCount;
static InsertTarget gTargets[kInsertTargetCapacity];

void astral_wire_set_node_inserter(AstralNodeInserter inserter)
{
    gInserter = inserter;
}

void astral_wire_set_insert_prefixes(const char *const *prefixes, size_t count)
{
    gPrefixCount = 0;
    for (size_t index = 0; index < count && gPrefixCount < kInsertPrefixCapacity; index++) {
        if (!prefixes[index] || !prefixes[index][0] || strlen(prefixes[index]) >= kInsertPrefixLength) continue;
        strcpy(gPrefixes[gPrefixCount++], prefixes[index]);
    }
}

static size_t prefixLength(const char *identifier)
{
    for (size_t index = 0; index < gPrefixCount; index++) {
        size_t length = strlen(gPrefixes[index]);
        if (strncmp(identifier, gPrefixes[index], length) == 0) return length;
    }
    return 0;
}

static InsertTarget *targetSlot(int32_t id, int create)
{
    size_t start = ((uint32_t)id * 2654435761u) % kInsertTargetCapacity;
    for (size_t probe = 0; probe < kInsertTargetCapacity; probe++) {
        InsertTarget *slot = &gTargets[(start + probe) % kInsertTargetCapacity];
        if (slot->id == id) return slot;
        if (slot->id == 0 || (create && slot->id < 0)) {
            if (!create) return NULL;
            memset(slot, 0, sizeof(*slot));
            slot->id = id;
            return slot;
        }
    }
    return NULL;
}

static void rememberChildren(const uint8_t *bytes, const Span *list, InsertTarget *target)
{
    target->childCount = 0;
    for (uint32_t position = 0; position < list->childCount && target->childCount < kTargetChildren; position++) {
        int32_t child = wireChildAt(bytes, list, position);
        int ours = 0;
        for (uint32_t index = 0; index < target->insertedCount; index++) ours |= target->inserted[index] == child;
        if (!ours) target->children[target->childCount++] = child;
    }
}

static const char *nodeType(AstralInsertKind kind)
{
    return kind == AstralInsertText ? "text" : kind == AstralInsertImage ? "image" : "view";
}

static uint16_t propertyCount(const AstralInsertNode *node)
{
    uint16_t count = 0;
    for (uint32_t field = node->fields; field; field &= field - 1) count++;
    if (node->fields & AstralInsertRow) count++;
    if (node->text[0] && node->kind == AstralInsertText) count++;
    if (node->uri[0] && node->kind == AstralInsertImage) count++;
    if (node->tap[0]) count += 2;
    return count;
}

static void putNode(Out *out, int32_t id, const AstralInsertNode *node)
{
    const char *type = nodeType(node->kind);
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)id);
    wirePut16(out, (uint16_t)strlen(type));
    wirePut(out, type, strlen(type));
    wirePropCount(out, propertyCount(node));
    if (node->kind == AstralInsertText && node->text[0]) wirePropString(out, P(text), node->text);
    if (node->kind == AstralInsertImage && node->uri[0]) wirePropString(out, P(uri), node->uri);
    if (node->fields & AstralInsertColor) wirePropLong(out, P(color), (int64_t)node->color);
    if (node->fields & AstralInsertTint) wirePropLong(out, P(tint), (int64_t)node->color);
    if (node->fields & AstralInsertBackground) wirePropLong(out, P(background), (int64_t)node->background);
    if (node->fields & AstralInsertRadius) wirePropNumber(out, P(radius), node->radius);
    if (node->fields & AstralInsertPadding)
        wirePropQuad(out, P(padding), node->padding[0], node->padding[1], node->padding[2], node->padding[3]);
    if (node->fields & AstralInsertMargin) wirePropQuad(out, P(margin), node->margin[0], node->margin[1], node->margin[2], node->margin[3]);
    if (node->fields & AstralInsertFontSize) wirePropNumber(out, P(fontSize), node->fontSize);
    if (node->fields & AstralInsertWidth) wirePropNumber(out, P(width), node->width);
    if (node->fields & AstralInsertHeight) wirePropNumber(out, P(height), node->height);
    if (node->fields & AstralInsertBold) wirePropInt(out, P(fontWeight), 600);
    if (node->fields & AstralInsertRow) {
        wirePropString(out, P(direction), "row");
        wirePropString(out, P(align), "center");
    }
    if (node->tap[0]) {
        wirePropString(out, P(accessibilityId), node->tap);
        wirePropEvents(out, P(on), "press");
    }
}

static void removeInserted(Out *out, InsertTarget *target)
{
    if (!target->insertedCount) return;
    wirePut8(out, 4);
    wirePut32(out, target->insertedCount);
    for (uint32_t index = 0; index < target->insertedCount; index++) wirePut32(out, (uint32_t)target->inserted[index]);
    target->insertedCount = 0;
    target->topLevel = 0;
}

uint32_t wireInsertedChildren(int32_t parent, int32_t *children, uint32_t capacity, int *atStart)
{
    InsertTarget *target = targetSlot(parent, 0);
    if (!target) return 0;
    uint32_t count = 0;
    for (uint32_t index = 0; index < target->topLevel && count < capacity; index++) children[count++] = target->inserted[index];
    *atStart = target->atStart;
    return count;
}

static void putTargetChildren(Out *out, const InsertTarget *target)
{
    int32_t children[kTargetChildren + kAstralInsertCapacity + 3];
    uint32_t count = wireLeadingChildren(target->id, children);
    if (target->atStart)
        for (uint32_t index = 0; index < target->topLevel; index++) children[count++] = target->inserted[index];
    for (uint32_t index = 0; index < target->childCount; index++) children[count++] = target->children[index];
    if (!target->atStart)
        for (uint32_t index = 0; index < target->topLevel; index++) children[count++] = target->inserted[index];
    count += wireTrailingChildren(target->id, children + count);
    wirePutChildren(out, target->id, children, count);
}

static void buildNodes(Out *out, InsertTarget *target, const AstralInsertNode *nodes, size_t count)
{
    int32_t ids[kAstralInsertCapacity];
    for (size_t index = 0; index < count; index++) {
        ids[index] = wireAllocateNode();
        putNode(out, ids[index], &nodes[index]);
    }
    int32_t top[kAstralInsertCapacity];
    uint32_t topCount = 0;
    for (size_t index = 0; index < count; index++) {
        if (nodes[index].parent >= 0 && (size_t)nodes[index].parent < index) continue;
        top[topCount++] = ids[index];
    }
    for (size_t parent = 0; parent < count; parent++) {
        int32_t children[kAstralInsertCapacity];
        uint32_t childCount = 0;
        for (size_t index = parent + 1; index < count; index++)
            if (nodes[index].parent == (int)parent) children[childCount++] = ids[index];
        if (childCount) wirePutChildren(out, ids[parent], children, childCount);
    }
    target->insertedCount = 0;
    for (uint32_t index = 0; index < topCount; index++) target->inserted[target->insertedCount++] = top[index];
    target->topLevel = topCount;
    for (size_t index = 0; index < count; index++) {
        int listed = 0;
        for (uint32_t known = 0; known < target->insertedCount; known++) listed |= target->inserted[known] == ids[index];
        if (!listed && target->insertedCount < kAstralInsertCapacity) target->inserted[target->insertedCount++] = ids[index];
    }
}

static int evaluate(Out *out, InsertTarget *target)
{
    AstralInsertNode nodes[kAstralInsertCapacity];
    memset(nodes, 0, sizeof(nodes));
    uint64_t message = wireMessageOfIdentifier(target->identifier);
    AstralPatchTarget context = {
        target->identifier,           target->type, prefixLength(target->identifier), message, message ? wireMessageAuthor(message) : 0,
        astral_wire_current_channel()};
    int atStart = 0;
    int count = gInserter(&context, nodes, kAstralInsertCapacity, &atStart);
    if (count < 0) {
        wireMarkRefreshBusy();
        return 0;
    }
    if (!count && !target->insertedCount) return 0;
    removeInserted(out, target);
    target->atStart = atStart;
    if (count) buildNodes(out, target, nodes, (size_t)count);
    putTargetChildren(out, target);
    return 1;
}

void wireInsertNodes(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gInserter || !gPrefixCount) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 4) {
            for (uint32_t position = 0; position < span->childCount; position++) {
                InsertTarget *removed = targetSlot(wireChildAt(bytes, span, position), 0);
                if (removed) removed->id = -1;
            }
            continue;
        }
        if (span->op == 3) {
            InsertTarget *target = targetSlot(span->id, 0);
            if (!target || wireCreation(spans, count, span->id)) continue;
            rememberChildren(bytes, span, target);
            if (!target->topLevel) continue;
            if (!*changed) wirePut(out, bytes, length);
            *changed = 1;
            putTargetChildren(out, target);
            continue;
        }
        if (span->op != 1) continue;
        char identifier[kWireIdentifierLength], type[kWireTypeLength];
        if (!wireReadIdentifier(bytes, span, identifier, type) || !prefixLength(identifier)) continue;
        InsertTarget *target = targetSlot(span->id, 1);
        if (!target) continue;
        strcpy(target->identifier, identifier);
        strcpy(target->type, type);
        const Span *list = wireChildList(bytes, spans, count, span->id);
        if (list) rememberChildren(bytes, list, target);
        Out staged = {0};
        if (!evaluate(&staged, target)) {
            free(staged.bytes);
            continue;
        }
        if (!*changed) wirePut(out, bytes, length);
        *changed = 1;
        wirePut(out, staged.bytes, staged.length);
        free(staged.bytes);
    }
}

int astral_wire_refresh_node_inserts(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    if (!gInserter) return 0;
    Out out = {0};
    wirePut8(&out, kWireVersion);
    int any = 0;
    for (size_t index = 0; index < kInsertTargetCapacity; index++) {
        if (gTargets[index].id > 0) any |= evaluate(&out, &gTargets[index]);
    }
    if (!any || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}
