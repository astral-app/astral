#include "PhoenixWireInternal.h"

#include <stdlib.h>
#include <string.h>

typedef struct {
    int32_t parent;
    uint32_t count;
    int32_t *children;
} ChildList;

enum { kEmptySlot = 0, kMaxListChildren = 1 << 16 };

static ChildList *gLists;
static size_t gCapacity;
static size_t gUsed;

static size_t slotOf(int32_t parent, size_t capacity)
{
    uint32_t hash = (uint32_t)parent * 2654435761u;
    return hash & (capacity - 1);
}

static ChildList *findList(int32_t parent)
{
    if (!gCapacity || parent == kEmptySlot) return NULL;
    for (size_t probe = 0, slot = slotOf(parent, gCapacity); probe < gCapacity; probe++, slot = (slot + 1) & (gCapacity - 1)) {
        if (gLists[slot].parent == parent) return &gLists[slot];
        if (gLists[slot].parent == kEmptySlot) return NULL;
    }
    return NULL;
}

static void placeList(ChildList *lists, size_t capacity, ChildList list)
{
    size_t slot = slotOf(list.parent, capacity);
    while (lists[slot].parent != kEmptySlot) slot = (slot + 1) & (capacity - 1);
    lists[slot] = list;
}

static int growLists(void)
{
    size_t capacity = gCapacity ? gCapacity * 2 : 1024;
    ChildList *lists = calloc(capacity, sizeof(*lists));
    if (!lists) return 0;
    for (size_t index = 0; index < gCapacity; index++)
        if (gLists[index].parent != kEmptySlot) placeList(lists, capacity, gLists[index]);
    free(gLists);
    gLists = lists;
    gCapacity = capacity;
    return 1;
}

static void forgetList(int32_t parent)
{
    ChildList *list = findList(parent);
    if (!list) return;
    free(list->children);
    size_t slot = (size_t)(list - gLists);
    gLists[slot].parent = kEmptySlot;
    gUsed--;
    for (size_t next = (slot + 1) & (gCapacity - 1); gLists[next].parent != kEmptySlot; next = (next + 1) & (gCapacity - 1)) {
        ChildList moved = gLists[next];
        gLists[next].parent = kEmptySlot;
        placeList(gLists, gCapacity, moved);
    }
}

static void rememberList(int32_t parent, int32_t *children, uint32_t count)
{
    if (parent == kEmptySlot) {
        free(children);
        return;
    }
    ChildList *list = findList(parent);
    if (list) {
        free(list->children);
        list->children = children;
        list->count = count;
        return;
    }
    if ((gUsed + 1) * 4 > gCapacity * 3 && !growLists()) {
        free(children);
        return;
    }
    placeList(gLists, gCapacity, (ChildList){parent, count, children});
    gUsed++;
}

static int32_t *copyChildren(const uint8_t *bytes, size_t offset, uint32_t count)
{
    int32_t *children = malloc((count ? count : 1) * sizeof(int32_t));
    if (children && count) memcpy(children, bytes + offset, (size_t)count * 4);
    return children;
}

static int32_t *applyEdits(const uint8_t *bytes, const Span *span, const ChildList *previous, uint32_t *resultCount)
{
    Cursor cursor = {bytes, span->end, span->start + 5, 0};
    uint32_t edits = wireRead32(&cursor);
    const int32_t *old = previous ? previous->children : NULL;
    uint32_t oldCount = previous ? previous->count : 0;
    size_t capacity = oldCount + 16;
    int32_t *children = malloc(capacity * sizeof(int32_t));
    uint32_t used = 0, position = 0;
    for (uint32_t edit = 0; edit < edits && children && !cursor.failed; edit++) {
        uint32_t keep = wireRead32(&cursor);
        uint32_t drop = wireRead32(&cursor);
        uint32_t added = wireRead32(&cursor);
        if (keep > oldCount - position || drop > oldCount - position - keep || added > kMaxListChildren ||
            !wireNeed(&cursor, (size_t)added * 4)) {
            cursor.failed = 1;
            break;
        }
        if (used + keep + added > capacity) {
            capacity = used + keep + added + oldCount;
            int32_t *grown = realloc(children, capacity * sizeof(int32_t));
            if (!grown) {
                cursor.failed = 1;
                break;
            }
            children = grown;
        }
        if (keep) memcpy(children + used, old + position, (size_t)keep * 4);
        used += keep;
        position += keep + drop;
        memcpy(children + used, bytes + cursor.offset, (size_t)added * 4);
        cursor.offset += (size_t)added * 4;
        used += added;
    }
    if (!children || cursor.failed || used + (oldCount - position) > kMaxListChildren) {
        free(children);
        return NULL;
    }
    if (used + (oldCount - position) > capacity) {
        int32_t *grown = realloc(children, (used + oldCount - position) * sizeof(int32_t));
        if (!grown) {
            free(children);
            return NULL;
        }
        children = grown;
    }
    if (oldCount > position) memcpy(children + used, old + position, (size_t)(oldCount - position) * 4);
    *resultCount = used + (oldCount - position);
    return children;
}

static void forgetRemoved(const uint8_t *bytes, const Span *span)
{
    for (uint32_t index = 0; index < span->childCount; index++) forgetList(wireChild(bytes, span, index));
}

uint8_t *wireExpandChildEdits(const uint8_t *bytes, size_t length, Span *spans, size_t *count, size_t *newLength)
{
    Out out = {0};
    size_t copied = 0;
    int expanded = 0;
    for (size_t index = 0; index < *count; index++) {
        const Span *span = &spans[index];
        if (span->op == 1) {
            forgetList(span->id);
        } else if (span->op == 3) {
            rememberList(span->id, copyChildren(bytes, span->children, span->childCount), span->childCount);
        } else if (span->op == 4) {
            forgetRemoved(bytes, span);
        } else if (span->op == 10) {
            uint32_t total = 0;
            int32_t *children = applyEdits(bytes, span, findList(span->id), &total);
            if (!children) {
                forgetList(span->id);
                continue;
            }
            if (!expanded)
                wirePut(&out, bytes, span->start);
            else
                wirePut(&out, bytes + copied, span->start - copied);
            wirePutChildren(&out, span->id, children, total);
            copied = span->end;
            expanded = 1;
            rememberList(span->id, children, total);
        }
    }
    if (!expanded) return NULL;
    wirePut(&out, bytes + copied, length - copied);
    if (out.failed || wireScan(out.bytes, out.length, spans, count) != 0) {
        free(out.bytes);
        wireScan(bytes, length, spans, count);
        return NULL;
    }
    *newLength = out.length;
    return out.bytes;
}

uint32_t wireDiscordChildren(int32_t parent, const int32_t **children)
{
    const ChildList *list = findList(parent);
    if (!list) return 0;
    *children = list->children;
    return list->count;
}

void wireResetChildLists(void)
{
    for (size_t index = 0; index < gCapacity; index++) free(gLists[index].children);
    free(gLists);
    gLists = NULL;
    gCapacity = 0;
    gUsed = 0;
}
