#include "PhoenixWireInternal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { kMessageNodeCapacity = 32768, kMaxMenuEntries = 12, kMaxSubtree = 256, kMaxGroupChildren = 64 };

static const int32_t kFirstMenuId = 0x7d000000;
static const char kActionPrefix[] = "message.actions.";

typedef struct {
    int32_t node;
    uint64_t message;
} MessageNode;

static MessageNode gMessageNodes[kMessageNodeCapacity];
static AstralMessageMenuProvider gMenuProvider;
static int32_t gNextMenuId = kFirstMenuId;

void astral_wire_set_message_menu_provider(AstralMessageMenuProvider provider)
{
    gMenuProvider = provider;
}

static MessageNode *messageSlot(int32_t node)
{
    return &gMessageNodes[((uint32_t)node * 2654435761u) % kMessageNodeCapacity];
}

uint64_t astral_wire_message_for_node(int32_t node)
{
    MessageNode *slot = messageSlot(node);
    return slot->node == node ? slot->message : 0;
}

static int stringAt(const uint8_t *bytes, const Span *span, uint16_t wanted, const uint8_t **text, uint32_t *length)
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

static uint64_t messageOf(const uint8_t *bytes, const Span *span)
{
    static const char *const prefixes[] = {"message.content:", "message.author:", "message.avatar:", "message.timestamp:"};
    const uint8_t *text;
    uint32_t length;
    if (!stringAt(bytes, span, P(accessibilityId), &text, &length)) return 0;
    for (size_t index = 0; index < sizeof(prefixes) / sizeof(prefixes[0]); index++) {
        uint64_t message = 0;
        if (wirePrefixed(text, length, prefixes[index], &message)) return message;
    }
    return 0;
}

static size_t subtree(const uint8_t *bytes, const Span *spans, size_t count, int32_t root, int32_t *nodes, size_t capacity)
{
    size_t used = 0, cursor = 0;
    nodes[used++] = root;
    while (cursor < used && used < capacity) {
        const Span *list = wireChildList(bytes, spans, count, nodes[cursor++]);
        for (uint32_t position = 0; list && position < list->childCount && used < capacity; position++)
            nodes[used++] = wireChildAt(bytes, list, position);
    }
    return used;
}

void wireTrackMessageNodes(const uint8_t *bytes, const Span *spans, size_t count)
{
    int32_t nodes[kMaxSubtree];
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 || !wireStringProperty(bytes, span, P(accessibilityId), "message-row")) continue;
        size_t found = subtree(bytes, spans, count, span->id, nodes, kMaxSubtree);
        uint64_t message = 0;
        for (size_t node = 0; node < found && !message; node++) {
            const Span *creation = wireCreation(spans, count, nodes[node]);
            if (creation) message = messageOf(bytes, creation);
        }
        if (!message) continue;
        for (size_t node = 0; node < found; node++) *messageSlot(nodes[node]) = (MessageNode){nodes[node], message};
    }
}

static int32_t menuId(void)
{
    int32_t id = gNextMenuId++;
    if (gNextMenuId >= kFirstMenuId + 0x00f00000) gNextMenuId = kFirstMenuId;
    return id;
}

static int isActionRow(const uint8_t *bytes, const Span *span)
{
    const uint8_t *text;
    uint32_t length;
    size_t prefix = sizeof(kActionPrefix) - 1;
    if (span->op != 1 || !stringAt(bytes, span, P(accessibilityId), &text, &length) || length <= prefix) return 0;
    if (memcmp(text, kActionPrefix, prefix) != 0) return 0;
    static const char *const skipped[] = {"quick_reaction", "add_reaction", "root"};
    for (size_t index = 0; index < sizeof(skipped) / sizeof(skipped[0]); index++) {
        size_t skippedLength = strlen(skipped[index]);
        if (length - prefix >= skippedLength && memcmp(text + prefix, skipped[index], skippedLength) == 0) return 0;
    }
    return 1;
}

static int hasIdentifier(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    const Span *creation = wireCreation(spans, count, id);
    const uint8_t *text;
    uint32_t length;
    return creation && stringAt(bytes, creation, P(accessibilityId), &text, &length) && length > 0;
}

static int32_t cloneNode(Out *out, const uint8_t *bytes, const Span *spans, size_t count, int32_t source, const Change *changes,
                         size_t changeCount)
{
    const Span *span = source ? wireCreation(spans, count, source) : NULL;
    if (!span) return 0;
    int32_t id = menuId();
    wireCloneCreation(out, bytes, span, id, changes, changeCount);
    return id;
}

static int32_t cloneTree(Out *out, const uint8_t *bytes, const Span *spans, size_t count, int32_t source, int depth)
{
    int32_t id = cloneNode(out, bytes, spans, count, source, NULL, 0);
    const Span *list = depth < 4 ? wireChildList(bytes, spans, count, source) : NULL;
    if (!id || !list || !list->childCount) return id;
    int32_t children[16];
    uint32_t used = 0;
    for (uint32_t position = 0; position < list->childCount && used < 16; position++) {
        int32_t child = cloneTree(out, bytes, spans, count, wireChildAt(bytes, list, position), depth + 1);
        if (child) children[used++] = child;
    }
    if (used) wirePutChildren(out, id, children, used);
    return id;
}

static int32_t buildRow(Out *out, const uint8_t *bytes, const Span *spans, size_t count, int32_t templateRow, const AstralMenuEntry *entry,
                        int first)
{
    const Span *rowList = wireChildList(bytes, spans, count, templateRow);
    int32_t content = rowList && rowList->childCount ? wireChildAt(bytes, rowList, 0) : 0;
    const Span *contentList = content ? wireChildList(bytes, spans, count, content) : NULL;
    if (!contentList || contentList->childCount < 2) return 0;
    int32_t icon = wireChildAt(bytes, contentList, 0), label = wireChildAt(bytes, contentList, 1);
    Change rowChanges[] = {{P(accessibilityId), kString, entry->action_id, 0},
                           {P(accessibilityLabel), kString, entry->label, 0},
                           first ? (Change){P(radiusTopLeft), kNumber, NULL, 12} : (Change){P(radiusTopLeft), kDrop, NULL, 0},
                           first ? (Change){P(radiusTopRight), kNumber, NULL, 12} : (Change){P(radiusTopRight), kDrop, NULL, 0},
                           {P(radiusBottomLeft), kDrop, NULL, 0},
                           {P(radiusBottomRight), kDrop, NULL, 0}};
    Change iconChange = {P(uri), kString, entry->icon, 0};
    Change labelChange = {P(text), kString, entry->label, 0};
    int32_t row = cloneNode(out, bytes, spans, count, templateRow, rowChanges, sizeof(rowChanges) / sizeof(*rowChanges));
    int32_t rowContent = cloneNode(out, bytes, spans, count, content, NULL, 0);
    int32_t rowIcon = cloneNode(out, bytes, spans, count, icon, entry->icon[0] ? &iconChange : NULL, entry->icon[0] ? 1 : 0);
    int32_t rowLabel = cloneNode(out, bytes, spans, count, label, &labelChange, 1);
    if (!row || !rowContent || !rowIcon || !rowLabel) return 0;
    wirePutChildren(out, row, (int32_t[]){rowContent}, 1);
    wirePutChildren(out, rowContent, (int32_t[]){rowIcon, rowLabel}, 2);
    return row;
}

static int32_t firstActionGroup(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++)
        if (isActionRow(bytes, &spans[index])) return wireParentOf(bytes, spans, count, spans[index].id);
    return 0;
}

static int32_t templateRowIn(const uint8_t *bytes, const Span *spans, size_t count)
{
    int32_t fallback = 0;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (!isActionRow(bytes, span)) continue;
        if (wireStringProperty(bytes, span, P(accessibilityId), "message.actions.copy_text")) return span->id;
        if (!fallback && !wireStringProperty(bytes, span, P(accessibilityId), "message.actions.delete")) fallback = span->id;
    }
    return fallback;
}

static int32_t separatorIn(const uint8_t *bytes, const Span *spans, size_t count, const Span *list)
{
    for (uint32_t position = 0; list && position < list->childCount; position++) {
        int32_t child = wireChildAt(bytes, list, position);
        if (!hasIdentifier(bytes, spans, count, child) && wireCreation(spans, count, child)) return child;
    }
    return 0;
}

static void putSquareTop(Out *out, int32_t row)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)row);
    wirePut16(out, 2);
    wirePropNumber(out, P(radiusTopLeft), 0);
    wirePropNumber(out, P(radiusTopRight), 0);
}

void wireMessageMenu(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gMenuProvider) return;
    int32_t templateRow = templateRowIn(bytes, spans, count);
    int32_t group = firstActionGroup(bytes, spans, count);
    const Span *groupList = group ? wireChildList(bytes, spans, count, group) : NULL;
    if (!templateRow || !groupList || !groupList->childCount || groupList->childCount > kMaxGroupChildren) return;
    AstralMenuEntry entries[kMaxMenuEntries];
    memset(entries, 0, sizeof(entries));
    size_t entryCount = gMenuProvider(entries, kMaxMenuEntries);
    if (!entryCount) return;
    int32_t separator = separatorIn(bytes, spans, count, groupList);
    if (!separator)
        separator = separatorIn(bytes, spans, count, wireChildList(bytes, spans, count, wireParentOf(bytes, spans, count, templateRow)));
    int32_t children[kMaxGroupChildren + kMaxMenuEntries * 2];
    uint32_t used = 0;
    Out update = {0};
    for (size_t index = 0; index < entryCount && index < kMaxMenuEntries; index++) {
        int32_t row = buildRow(&update, bytes, spans, count, templateRow, &entries[index], index == 0);
        if (!row) break;
        children[used++] = row;
        int32_t divider = separator ? cloneTree(&update, bytes, spans, count, separator, 0) : 0;
        if (divider) children[used++] = divider;
    }
    if (update.failed || !used) {
        free(update.bytes);
        return;
    }
    int32_t firstOriginal = wireChildAt(bytes, groupList, 0);
    if (hasIdentifier(bytes, spans, count, firstOriginal)) putSquareTop(&update, firstOriginal);
    for (uint32_t position = 0; position < groupList->childCount; position++) children[used++] = wireChildAt(bytes, groupList, position);
    wirePutChildren(&update, group, children, used);
    if (!*changed) wirePut(out, bytes, length);
    *changed = 1;
    wirePut(out, update.bytes, update.length);
    free(update.bytes);
}
