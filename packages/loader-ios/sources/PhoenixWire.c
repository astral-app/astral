#include "PhoenixWire.h"

#include "PhoenixProperties.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { kMaxSpans = 8192, kMaxChildren = 4096, kMaxProps = 128 };

static const char kAnchor[] = "settings.row.account";
static const int32_t kFirstId = 0x7a000000;

typedef struct {
    uint8_t op;
    int32_t id;
    size_t start;
    size_t end;
    size_t children;
    uint32_t childCount;
} Span;

typedef struct {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
    int failed;
} Cursor;

typedef struct {
    uint8_t *bytes;
    size_t length;
    size_t capacity;
    int failed;
} Out;

static size_t length_of_batch;

enum { kMaxCategoryNodes = 128, kMaxCategoryRows = 8, kCategoryText = 96 };

static struct {
    int32_t list;
    int32_t ids[kMaxCategoryNodes];
    uint32_t count;
    int active;
} gState;

static struct {
    char key[kCategoryText];
    char title[kCategoryText];
    char icon[kCategoryText];
} gCategoryRows[kMaxCategoryRows] = {{"", "astral", "res://ic_mana_sparkles"}};
static size_t gCategoryRowCount = 1;
static char gCategoryTitle[kCategoryText] = "astral";

static int need(Cursor *cursor, size_t count)
{
    if (cursor->failed || cursor->length - cursor->offset < count) {
        cursor->failed = 1;
        return 0;
    }
    return 1;
}

static uint32_t read32(Cursor *cursor)
{
    if (!need(cursor, 4)) return 0;
    uint32_t value;
    memcpy(&value, cursor->bytes + cursor->offset, 4);
    cursor->offset += 4;
    return value;
}

static uint16_t read16(Cursor *cursor)
{
    if (!need(cursor, 2)) return 0;
    uint16_t value = (uint16_t)(cursor->bytes[cursor->offset] | cursor->bytes[cursor->offset + 1] << 8);
    cursor->offset += 2;
    return value;
}

static uint8_t read8(Cursor *cursor)
{
    return need(cursor, 1) ? cursor->bytes[cursor->offset++] : 0;
}

static void skip(Cursor *cursor, size_t count)
{
    if (need(cursor, count)) cursor->offset += count;
}

static void skipValue(Cursor *cursor, uint8_t tag, int depth);

static void skipTagged(Cursor *cursor, int depth)
{
    skipValue(cursor, read8(cursor), depth);
}

static void skipValue(Cursor *cursor, uint8_t tag, int depth)
{
    if (depth > 8) {
        cursor->failed = 1;
        return;
    }
    switch (tag) {
    case 0:
    case 1:
    case 2:
        break;
    case 3:
        skip(cursor, 4);
        break;
    case 4:
    case 5:
        skip(cursor, 8);
        break;
    case 6:
        skip(cursor, read32(cursor));
        break;
    case 7: {
        uint32_t count = read32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) skipTagged(cursor, depth + 1);
        break;
    }
    case 8: {
        uint32_t count = read32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            skip(cursor, 2);
            skipTagged(cursor, depth + 1);
        }
        break;
    }
    case 9:
        skip(cursor, 16);
        break;
    case 10: {
        uint32_t count = read32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            skip(cursor, read32(cursor));
            skipTagged(cursor, depth + 1);
        }
        break;
    }
    case 11:
        skip(cursor, (size_t)read32(cursor) * 4);
        break;
    case 12: {
        skip(cursor, 4);
        skip(cursor, (size_t)read16(cursor) * 6);
        break;
    }
    default:
        cursor->failed = 1;
        break;
    }
}

static void skipProperties(Cursor *cursor)
{
    uint16_t count = read16(cursor);
    for (uint16_t index = 0; index < count && !cursor->failed; index++) {
        skip(cursor, 2);
        skipValue(cursor, read8(cursor), 0);
    }
}

static int scan(const uint8_t *bytes, size_t length, Span *spans, size_t *count)
{
    Cursor cursor = {bytes, length, 0, 0};
    if (read8(&cursor) != 1) return -1;
    size_t used = 0;
    while (!cursor.failed && cursor.offset < length) {
        if (used == kMaxSpans) return -1;
        Span *span = &spans[used++];
        memset(span, 0, sizeof(*span));
        span->start = cursor.offset;
        span->op = read8(&cursor);
        switch (span->op) {
        case 1:
            span->id = (int32_t)read32(&cursor);
            skip(&cursor, read16(&cursor));
            skipProperties(&cursor);
            break;
        case 2:
            span->id = (int32_t)read32(&cursor);
            skipProperties(&cursor);
            break;
        case 3:
            span->id = (int32_t)read32(&cursor);
            span->childCount = read32(&cursor);
            span->children = cursor.offset;
            skip(&cursor, (size_t)span->childCount * 4);
            break;
        case 4:
        case 9:
            span->childCount = read32(&cursor);
            span->children = cursor.offset;
            skip(&cursor, (size_t)span->childCount * 4);
            break;
        case 5:
            skip(&cursor, 4);
            break;
        case 6:
            if (read8(&cursor)) skip(&cursor, 4);
            break;
        case 7:
            skip(&cursor, 4);
            skipTagged(&cursor, 0);
            break;
        case 8:
            break;
        case 10: {
            span->id = (int32_t)read32(&cursor);
            uint32_t edits = read32(&cursor);
            for (uint32_t index = 0; index < edits && !cursor.failed; index++) {
                skip(&cursor, 8);
                skip(&cursor, (size_t)read32(&cursor) * 4);
            }
            break;
        }
        default:
            return -1;
        }
        span->end = cursor.offset;
    }
    *count = used;
    return cursor.failed ? -1 : 0;
}

static int32_t child_at(const uint8_t *bytes, const Span *span, uint32_t index);
static int32_t child(const uint8_t *bytes, const Span *span, uint32_t index)
{
    int32_t value;
    memcpy(&value, bytes + span->children + (size_t)index * 4, 4);
    return value;
}

static int32_t child_at(const uint8_t *bytes, const Span *span, uint32_t index)
{
    return child(bytes, span, index);
}

static const Span *childList(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    (void)bytes;
    const Span *found = NULL;
    for (size_t index = 0; index < count; index++) {
        if (spans[index].op == 3 && spans[index].id == id) found = &spans[index];
    }
    return found;
}

static int32_t parentOf(const uint8_t *bytes, const Span *spans, size_t count, int32_t id)
{
    for (size_t index = count; index-- > 0;) {
        const Span *span = &spans[index];
        if (span->op != 3) continue;
        for (uint32_t position = 0; position < span->childCount; position++) {
            if (child(bytes, span, position) == id) return span->id;
        }
    }
    return 0;
}

static const Span *creation(const Span *spans, size_t count, int32_t id)
{
    for (size_t index = 0; index < count; index++) {
        if (spans[index].op == 1 && spans[index].id == id) return &spans[index];
    }
    return NULL;
}

static int stringProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, const char *value)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    skip(&cursor, read16(&cursor));
    uint16_t count = read16(&cursor);
    size_t valueLength = strlen(value);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        if (property == wanted && tag == 6) {
            uint32_t length = read32(&cursor);
            if (!need(&cursor, length)) return 0;
            return length == valueLength && memcmp(cursor.bytes + cursor.offset, value, length) == 0;
        }
        skipValue(&cursor, tag, 0);
    }
    return 0;
}

static void reserve(Out *out, size_t more)
{
    if (out->failed) return;
    if (out->length + more <= out->capacity) return;
    size_t capacity = out->capacity ? out->capacity : 4096;
    while (capacity < out->length + more) capacity *= 2;
    uint8_t *grown = realloc(out->bytes, capacity);
    if (!grown) {
        out->failed = 1;
        return;
    }
    out->bytes = grown;
    out->capacity = capacity;
}

static void put(Out *out, const void *bytes, size_t length)
{
    reserve(out, length);
    if (out->failed) return;
    memcpy(out->bytes + out->length, bytes, length);
    out->length += length;
}

static void put8(Out *out, uint8_t value)
{
    put(out, &value, 1);
}
static void put16(Out *out, uint16_t value)
{
    put(out, &value, 2);
}
static void put32(Out *out, uint32_t value)
{
    put(out, &value, 4);
}

typedef enum { kKeep, kDrop, kString, kNumber, kInteger } Edit;

typedef struct {
    uint16_t property;
    Edit edit;
    const char *text;
    double number;
} Change;

static const Change *changeFor(const Change *changes, size_t count, uint16_t property)
{
    for (size_t index = 0; index < count; index++) {
        if (changes[index].property == property) return &changes[index];
    }
    return NULL;
}

static void putValue(Out *out, const Change *change)
{
    if (change->edit == kString) {
        size_t length = strlen(change->text);
        put8(out, 6);
        put32(out, (uint32_t)length);
        put(out, change->text, length);
    } else if (change->edit == kInteger) {
        put8(out, 3);
        put32(out, (uint32_t)(int32_t)change->number);
    } else {
        put8(out, 5);
        put(out, &change->number, 8);
    }
}

static void cloneCreation(Out *out, const uint8_t *bytes, const Span *span, int32_t id, const Change *changes, size_t changeCount)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    uint16_t typeLength = read16(&cursor);
    size_t typeStart = cursor.offset;
    skip(&cursor, typeLength);
    uint16_t count = read16(&cursor);
    if (cursor.failed) {
        out->failed = 1;
        return;
    }
    put8(out, 1);
    put32(out, (uint32_t)id);
    put16(out, typeLength);
    put(out, bytes + typeStart, typeLength);
    size_t countAt = out->length;
    put16(out, 0);
    uint16_t written = 0;
    int seen[kMaxProps] = {0};
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        size_t start = cursor.offset;
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        skipValue(&cursor, tag, 0);
        const Change *change = changeFor(changes, changeCount, property);
        if (change) seen[change - changes] = 1;
        if (change && change->edit == kDrop) continue;
        if (!change && tag == 12) continue;
        if (change && change->edit != kKeep) {
            put16(out, property);
            putValue(out, change);
        } else {
            put(out, bytes + start, cursor.offset - start);
        }
        written++;
    }
    for (size_t index = 0; index < changeCount && index < kMaxProps; index++) {
        if (seen[index] || changes[index].edit == kDrop || changes[index].edit == kKeep) continue;
        put16(out, changes[index].property);
        putValue(out, &changes[index]);
        written++;
    }
    if (cursor.failed) out->failed = 1;
    if (!out->failed) memcpy(out->bytes + countAt, &written, 2);
}

static void putChildren(Out *out, int32_t parent, const int32_t *children, uint32_t count)
{
    put8(out, 3);
    put32(out, (uint32_t)parent);
    put32(out, count);
    for (uint32_t index = 0; index < count; index++) put32(out, (uint32_t)children[index]);
}

static void putListWithAstral(Out *out, const uint8_t *bytes, const Span *list)
{
    int32_t children[kMaxChildren];
    uint32_t count = 0;
    for (uint32_t index = 0; index < list->childCount && count < kMaxChildren - 1; index++) {
        int32_t id = child(bytes, list, index);
        if (id == gState.ids[0]) continue;
        children[count++] = id;
        if (index == 0) children[count++] = gState.ids[0];
    }
    putChildren(out, list->id, children, count);
}

static int removes(const uint8_t *bytes, const Span *span, int32_t id)
{
    for (uint32_t index = 0; index < span->childCount; index++) {
        if (child(bytes, span, index) == id) return 1;
    }
    return 0;
}

static void putRemoval(Out *out)
{
    put8(out, 4);
    put32(out, gState.count);
    for (uint32_t index = 0; index < gState.count; index++) put32(out, (uint32_t)gState.ids[index]);
}

static void copyText(char *destination, const char *source)
{
    snprintf(destination, kCategoryText, "%s", source ? source : "");
}

void astral_wire_set_category(const char *title, const AstralRowSpec *rows, size_t count)
{
    copyText(gCategoryTitle, title);
    gCategoryRowCount = count < kMaxCategoryRows ? count : kMaxCategoryRows;
    for (size_t index = 0; index < gCategoryRowCount; index++) {
        copyText(gCategoryRows[index].key, rows[index].key);
        copyText(gCategoryRows[index].title, rows[index].title);
        copyText(gCategoryRows[index].icon, rows[index].icon);
    }
}

static int32_t categoryNode(Out *out, const uint8_t *bytes, const Span *spans, size_t count, int32_t source, const Change *changes,
                            size_t changeCount)
{
    const Span *span = creation(spans, count, source);
    if (!span || gState.count >= kMaxCategoryNodes) {
        out->failed = 1;
        return 0;
    }
    int32_t id = kFirstId + (int32_t)gState.count;
    gState.ids[gState.count++] = id;
    cloneCreation(out, bytes, span, id, changes, changeCount);
    return id;
}

static int32_t categoryRow(Out *out, const uint8_t *bytes, const Span *spans, size_t count, const int32_t *source, size_t index)
{
    char identifier[kCategoryText + 32];
    if (gCategoryRows[index].key[0]) {
        snprintf(identifier, sizeof(identifier), "settings.row.astral.%s", gCategoryRows[index].key);
    } else {
        snprintf(identifier, sizeof(identifier), "settings.row.astral");
    }
    int first = index == 0, last = index + 1 == gCategoryRowCount;
    Change rowChanges[] = {{P(accessibilityId), kString, identifier, 0},
                           {P(accessibilityLabel), kString, gCategoryRows[index].title, 0},
                           last ? (Change){P(radiusBottomLeft), kNumber, NULL, 12} : (Change){P(radiusBottomLeft), kDrop, NULL, 0},
                           last ? (Change){P(radiusBottomRight), kNumber, NULL, 12} : (Change){P(radiusBottomRight), kDrop, NULL, 0},
                           first ? (Change){P(radiusTopLeft), kNumber, NULL, 12} : (Change){P(radiusTopLeft), kDrop, NULL, 0},
                           first ? (Change){P(radiusTopRight), kNumber, NULL, 12} : (Change){P(radiusTopRight), kDrop, NULL, 0}};
    Change iconChange = {P(uri), kString, gCategoryRows[index].icon, 0};
    Change titleChange = {P(text), kString, gCategoryRows[index].title, 0};
    int32_t row = categoryNode(out, bytes, spans, count, source[0], rowChanges, sizeof(rowChanges) / sizeof(*rowChanges));
    int32_t content = categoryNode(out, bytes, spans, count, source[1], NULL, 0);
    int32_t icon = categoryNode(out, bytes, spans, count, source[2], &iconChange, 1);
    int32_t title = categoryNode(out, bytes, spans, count, source[3], &titleChange, 1);
    int32_t trailing = categoryNode(out, bytes, spans, count, source[4], NULL, 0);
    int32_t chevron = categoryNode(out, bytes, spans, count, source[5], NULL, 0);
    putChildren(out, row, (int32_t[]){content}, 1);
    putChildren(out, content, (int32_t[]){icon, title, trailing}, 3);
    putChildren(out, trailing, (int32_t[]){chevron}, 1);
    return row;
}

static int createCategory(Out *out, const uint8_t *bytes, const Span *spans, size_t count, const Span *row)
{
    int32_t card = parentOf(bytes, spans, count, row->id);
    int32_t group = card ? parentOf(bytes, spans, count, card) : 0;
    int32_t list = group ? parentOf(bytes, spans, count, group) : 0;
    const Span *groupChildren = group ? childList(bytes, spans, count, group) : NULL;
    const Span *cardChildren = card ? childList(bytes, spans, count, card) : NULL;
    const Span *rowChildren = childList(bytes, spans, count, row->id);
    if (!list || !groupChildren || groupChildren->childCount < 2 || !rowChildren || rowChildren->childCount < 1 || !cardChildren) return 0;
    int32_t header = child(bytes, groupChildren, 0);
    int32_t content = child(bytes, rowChildren, 0);
    const Span *contentChildren = childList(bytes, spans, count, content);
    if (!contentChildren || contentChildren->childCount < 3) return 0;
    int32_t trailing = child(bytes, contentChildren, 2);
    const Span *trailingChildren = childList(bytes, spans, count, trailing);
    if (!trailingChildren || trailingChildren->childCount < 1) return 0;
    int32_t source[6] = {
        row->id, content, child(bytes, contentChildren, 0), child(bytes, contentChildren, 1), trailing, child(bytes, trailingChildren, 0)};
    int32_t separator = cardChildren->childCount > 1 ? child(bytes, cardChildren, 1) : 0;
    const Span *separatorChildren = separator ? childList(bytes, spans, count, separator) : NULL;
    int32_t separatorLine = separatorChildren && separatorChildren->childCount ? child(bytes, separatorChildren, 0) : 0;
    if (gCategoryRowCount > 1 && (!separator || !separatorLine)) return 0;

    gState.count = 0;
    Change headerChange = {P(text), kString, gCategoryTitle, 0};
    int32_t newGroup = categoryNode(out, bytes, spans, count, group, NULL, 0);
    int32_t newHeader = categoryNode(out, bytes, spans, count, header, &headerChange, 1);
    int32_t newCard = categoryNode(out, bytes, spans, count, card, NULL, 0);
    int32_t children[kMaxCategoryRows * 2];
    uint32_t childCount = 0;
    for (size_t index = 0; index < gCategoryRowCount; index++) {
        if (index) {
            int32_t newSeparator = categoryNode(out, bytes, spans, count, separator, NULL, 0);
            int32_t newLine = categoryNode(out, bytes, spans, count, separatorLine, NULL, 0);
            putChildren(out, newSeparator, (int32_t[]){newLine}, 1);
            children[childCount++] = newSeparator;
        }
        children[childCount++] = categoryRow(out, bytes, spans, count, source, index);
    }
    if (out->failed) return 0;
    putChildren(out, newGroup, (int32_t[]){newHeader, newCard}, 2);
    putChildren(out, newCard, children, childCount);
    gState.list = list;
    gState.active = 1;
    putListWithAstral(out, bytes, childList(bytes, spans, count, list));
    return 1;
}

enum Role {
    kRoot,
    kPhone,
    kColumn,
    kContainer,
    kSpacer,
    kHairline,
    kScroll,
    kList,
    kHeaderBar,
    kHeaderLeft,
    kHeaderLeftInner,
    kHeaderLeftPad,
    kBack,
    kBackStack,
    kBackStackInner,
    kBackIcon,
    kTitleWrap,
    kTitleInner,
    kTitle,
    kGroup,
    kGroupHeader,
    kCard,
    kRow,
    kRowContent,
    kRowIcon,
    kRowTitle,
    kRowTrailing,
    kRowChevron,
    kSeparator,
    kSeparatorLine,
    kRoleCount
};

enum { kMaxNavigatorChildren = 64, kMaxPageNodes = 1024 };

static const int32_t kFirstPageId = 0x7a000100;

static struct {
    uint8_t *bytes;
    size_t length;
} gTemplates[kRoleCount];

static int gTemplatesReady;

typedef struct {
    int32_t id;
    int32_t children[kMaxNavigatorChildren];
    uint32_t count;
    int32_t index;
} Navigator;

enum { kMaxNavigators = 8 };

static Navigator gNavigators[kMaxNavigators];
static uint32_t gNavigatorCount;
static int32_t gSettingsNavigator;

static Navigator *navigatorWithId(int32_t id)
{
    for (uint32_t index = 0; index < gNavigatorCount; index++) {
        if (gNavigators[index].id == id) return &gNavigators[index];
    }
    return NULL;
}

enum { kMaxPages = 4, kPageIdStride = 0x40000 };

typedef struct {
    int32_t ids[kMaxPageNodes];
    uint32_t count;
    int32_t firstId;
    int32_t nextId;
    int32_t root;
    int32_t list;
    uint32_t contentStart;
    uint32_t contentEnd;
    int32_t previousChildren[kMaxNavigatorChildren];
    uint32_t previousCount;
    int32_t previousIndex;
} Page;

static Page gPages[kMaxPages];
static uint32_t gDepth;
static Page *gPage = &gPages[0];

static int32_t firstChild(const uint8_t *bytes, const Span *spans, size_t count, int32_t id, uint32_t position)
{
    const Span *list = childList(bytes, spans, count, id);
    return list && list->childCount > position ? child(bytes, list, position) : 0;
}

static int storeTemplate(const uint8_t *bytes, const Span *spans, size_t count, int role, int32_t id)
{
    const Span *span = id ? creation(spans, count, id) : NULL;
    if (!span) return 0;
    free(gTemplates[role].bytes);
    gTemplates[role].length = span->end - span->start;
    gTemplates[role].bytes = malloc(gTemplates[role].length);
    if (!gTemplates[role].bytes) return 0;
    memcpy(gTemplates[role].bytes, bytes + span->start, gTemplates[role].length);
    return 1;
}

static void captureTemplates(const uint8_t *bytes, const Span *spans, size_t count, int32_t row)
{
    int32_t ids[kRoleCount] = {0};
    ids[kRow] = row;
    ids[kCard] = parentOf(bytes, spans, count, row);
    ids[kGroup] = ids[kCard] ? parentOf(bytes, spans, count, ids[kCard]) : 0;
    ids[kList] = ids[kGroup] ? parentOf(bytes, spans, count, ids[kGroup]) : 0;
    ids[kScroll] = ids[kList] ? parentOf(bytes, spans, count, ids[kList]) : 0;
    ids[kContainer] = ids[kScroll] ? parentOf(bytes, spans, count, ids[kScroll]) : 0;
    ids[kColumn] = ids[kContainer] ? parentOf(bytes, spans, count, ids[kContainer]) : 0;
    ids[kPhone] = ids[kColumn] ? parentOf(bytes, spans, count, ids[kColumn]) : 0;
    ids[kRoot] = ids[kPhone] ? parentOf(bytes, spans, count, ids[kPhone]) : 0;
    ids[kSpacer] = firstChild(bytes, spans, count, ids[kContainer], 0);
    ids[kHairline] = firstChild(bytes, spans, count, ids[kContainer], 1);
    if (firstChild(bytes, spans, count, ids[kContainer], 2) != ids[kScroll]) return;
    ids[kHeaderBar] = firstChild(bytes, spans, count, ids[kContainer], 3);
    ids[kHeaderLeft] = firstChild(bytes, spans, count, ids[kHeaderBar], 0);
    ids[kHeaderLeftInner] = firstChild(bytes, spans, count, ids[kHeaderLeft], 0);
    ids[kHeaderLeftPad] = firstChild(bytes, spans, count, ids[kHeaderLeftInner], 0);
    ids[kBack] = firstChild(bytes, spans, count, ids[kHeaderLeftPad], 0);
    ids[kBackStack] = firstChild(bytes, spans, count, ids[kBack], 0);
    ids[kBackStackInner] = firstChild(bytes, spans, count, ids[kBackStack], 0);
    ids[kBackIcon] = firstChild(bytes, spans, count, ids[kBackStackInner], 0);
    ids[kTitleWrap] = firstChild(bytes, spans, count, ids[kHeaderBar], 1);
    ids[kTitleInner] = firstChild(bytes, spans, count, ids[kTitleWrap], 0);
    ids[kTitle] = firstChild(bytes, spans, count, ids[kTitleInner], 0);
    ids[kGroupHeader] = firstChild(bytes, spans, count, ids[kGroup], 0);
    ids[kSeparator] = firstChild(bytes, spans, count, ids[kCard], 1);
    ids[kSeparatorLine] = firstChild(bytes, spans, count, ids[kSeparator], 0);
    ids[kRowContent] = firstChild(bytes, spans, count, row, 0);
    ids[kRowIcon] = firstChild(bytes, spans, count, ids[kRowContent], 0);
    ids[kRowTitle] = firstChild(bytes, spans, count, ids[kRowContent], 1);
    ids[kRowTrailing] = firstChild(bytes, spans, count, ids[kRowContent], 2);
    ids[kRowChevron] = firstChild(bytes, spans, count, ids[kRowTrailing], 0);
    int32_t navigator = parentOf(bytes, spans, count, ids[kRoot]);
    if (!navigator || !navigatorWithId(navigator)) return;
    for (int role = 0; role < kRoleCount; role++) {
        if (!storeTemplate(bytes, spans, count, role, ids[role])) return;
    }
    gSettingsNavigator = navigator;
    gTemplatesReady = 1;
}

static int typeIs(const uint8_t *bytes, const Span *span, const char *type)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    uint16_t length = read16(&cursor);
    return need(&cursor, length) && length == strlen(type) && memcmp(bytes + cursor.offset, type, length) == 0;
}

static int32_t integerProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, int *found)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    uint16_t count = read16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        if (property == wanted && tag == 3) {
            *found = 1;
            return (int32_t)read32(&cursor);
        }
        skipValue(&cursor, tag, 0);
    }
    return 0;
}

static void putPageRemoval(Out *out, const Page *page)
{
    put8(out, 4);
    put32(out, page->count);
    for (uint32_t index = 0; index < page->count; index++) put32(out, (uint32_t)page->ids[index]);
}

static void putAllPagesRemoval(Out *out)
{
    for (uint32_t index = gDepth; index > 0; index--) putPageRemoval(out, &gPages[index - 1]);
    gDepth = 0;
}

static int trackNavigator(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 1 && typeIs(bytes, span, "navigator") && !navigatorWithId(span->id) && gNavigatorCount < kMaxNavigators) {
            gNavigators[gNavigatorCount++] = (Navigator){.id = span->id};
        }
        Navigator *navigator = (span->op == 2 || span->op == 3) ? navigatorWithId(span->id) : NULL;
        if (!navigator) continue;
        if (span->op == 3 && span->childCount <= kMaxNavigatorChildren) {
            navigator->count = span->childCount;
            for (uint32_t position = 0; position < span->childCount; position++)
                navigator->children[position] = child(bytes, span, position);
            if (gDepth && navigator->id == gSettingsNavigator) {
                if (!*changed) put(out, bytes, length_of_batch);
                putAllPagesRemoval(out);
                *changed = 1;
            }
        }
        if (span->op == 2) {
            int found = 0;
            int32_t value = integerProperty(bytes, span, P(index), &found);
            if (found) navigator->index = value;
        }
    }
    return 0;
}

static int32_t allocate(void)
{
    if (gPage->count == 0) gPage->nextId = gPage->firstId;
    int32_t id = gPage->nextId++;
    if (gPage->count < kMaxPageNodes) gPage->ids[gPage->count++] = id;
    return id;
}

static int32_t cloneRole(Out *out, int role, const Change *changes, size_t changeCount)
{
    Span span = {1, 0, 0, gTemplates[role].length, 0, 0};
    int32_t id = allocate();
    cloneCreation(out, gTemplates[role].bytes, &span, id, changes, changeCount);
    return id;
}

static int32_t newNode(Out *out, const char *type)
{
    int32_t id = allocate();
    size_t length = strlen(type);
    put8(out, 1);
    put32(out, (uint32_t)id);
    put16(out, (uint16_t)length);
    put(out, type, length);
    return id;
}

static void propCount(Out *out, uint16_t count)
{
    put16(out, count);
}
static void propString(Out *out, uint16_t property, const char *value)
{
    size_t length = strlen(value);
    put16(out, property);
    put8(out, 6);
    put32(out, (uint32_t)length);
    put(out, value, length);
}
static void propNumber(Out *out, uint16_t property, double value)
{
    put16(out, property);
    put8(out, 5);
    put(out, &value, 8);
}
static void propInt(Out *out, uint16_t property, int32_t value)
{
    put16(out, property);
    put8(out, 3);
    put32(out, (uint32_t)value);
}
static void propLong(Out *out, uint16_t property, int64_t value)
{
    put16(out, property);
    put8(out, 4);
    put(out, &value, 8);
}
static void propBool(Out *out, uint16_t property, int value)
{
    put16(out, property);
    put8(out, value ? 2 : 1);
}
static void propQuad(Out *out, uint16_t property, float a, float b, float c, float d)
{
    float quad[4] = {a, b, c, d};
    put16(out, property);
    put8(out, 9);
    put(out, quad, sizeof(quad));
}
static void propEvents(Out *out, uint16_t property, const char *event)
{
    size_t length = strlen(event);
    put16(out, property);
    put8(out, 7);
    put32(out, 1);
    put8(out, 6);
    put32(out, (uint32_t)length);
    put(out, event, length);
}

static const int64_t kSwitchOn = 0xFF5865F2;
static const int32_t kSwitchOff = 0x1F000000;
static const int64_t kThumb = 0xFFFFFFFF;
static const int32_t kSwitchBorder = 530028447;

static int32_t buildSwitch(Out *out, const char *key, int on)
{
    char identifier[128];
    snprintf(identifier, sizeof(identifier), "astral.switch.%s", key);
    int32_t track = newNode(out, "view");
    propCount(out, P(accessibilityTapLabel));
    propBool(out, P(accessibilityHidden), 1);
    propString(out, P(accessibilityId), identifier);
    propString(out, P(align), "stretch");
    if (on)
        propLong(out, P(background), kSwitchOn);
    else
        propInt(out, P(background), kSwitchOff);
    propInt(out, P(borderColor), kSwitchBorder);
    propNumber(out, P(borderWidth), 1);
    propString(out, P(direction), "row");
    propNumber(out, P(height), 28);
    propEvents(out, P(on), "press");
    propQuad(out, P(padding), 4, 4, 4, 4);
    propString(out, P(pressFeedback), "none");
    propNumber(out, P(radius), 16);
    propNumber(out, P(width), 56);
    propBool(out, P(exactHitArea), 1);
    int32_t thumb = newNode(out, "view");
    propCount(out, P(accessibilityHint));
    propString(out, P(align), "center");
    propLong(out, P(background), kThumb);
    propString(out, P(direction), "row");
    propNumber(out, P(height), 20);
    propString(out, P(justify), "center");
    propNumber(out, P(radius), 9999);
    propNumber(out, P(translateX), on ? 28 : 0);
    propNumber(out, P(width), 20);
    putChildren(out, track, (int32_t[]){thumb}, 1);
    return track;
}

static int32_t buildSpecRow(Out *out, const AstralRowSpec *row, int first, int last)
{
    char identifier[128];
    snprintf(identifier, sizeof(identifier), "astral.row.%s", row->key);
    Change rowChanges[12];
    size_t used = 0;
    rowChanges[used++] = (Change){P(accessibilityId), kString, identifier, 0};
    rowChanges[used++] = (Change){P(accessibilityLabel), kString, row->title, 0};
    rowChanges[used++] = first ? (Change){P(radiusTopLeft), kNumber, NULL, 12} : (Change){P(radiusTopLeft), kDrop, NULL, 0};
    rowChanges[used++] = first ? (Change){P(radiusTopRight), kNumber, NULL, 12} : (Change){P(radiusTopRight), kDrop, NULL, 0};
    rowChanges[used++] = last ? (Change){P(radiusBottomLeft), kNumber, NULL, 12} : (Change){P(radiusBottomLeft), kDrop, NULL, 0};
    rowChanges[used++] = last ? (Change){P(radiusBottomRight), kNumber, NULL, 12} : (Change){P(radiusBottomRight), kDrop, NULL, 0};
    if (row->kind == kAstralRowInfo) rowChanges[used++] = (Change){P(on), kDrop, NULL, 0};
    if (row->kind == kAstralRowSwitch) {
        rowChanges[used++] = (Change){P(on), kDrop, NULL, 0};
        rowChanges[used++] = (Change){P(role), kString, "switch", 0};
    }
    int32_t rowId = cloneRole(out, kRow, rowChanges, used);
    int32_t content = cloneRole(out, kRowContent, NULL, 0);
    Change iconChange = {P(uri), kString, row->icon ? row->icon : "res://ic_mana_sparkles", 0};
    int32_t icon = cloneRole(out, kRowIcon, &iconChange, 1);
    Change titleChange = {P(text), kString, row->title, 0};
    int32_t title = cloneRole(out, kRowTitle, &titleChange, 1);
    if (row->subtitle && row->subtitle[0]) {
        int32_t column = newNode(out, "view");
        propCount(out, P(accessibilityChecked));
        propString(out, P(direction), "column");
        propNumber(out, P(flex), 1);
        propNumber(out, P(gap), 2);
        Change subtitleChanges[] = {{P(text), kString, row->subtitle, 0},
                                    {P(fontSize), kNumber, NULL, 13},
                                    {P(lineHeight), kNumber, NULL, 17},
                                    {P(opacity), kNumber, NULL, 0.62},
                                    {P(maxLines), kInteger, NULL, 3}};
        int32_t subtitle = cloneRole(out, kRowTitle, subtitleChanges, 5);
        putChildren(out, column, (int32_t[]){title, subtitle}, 2);
        title = column;
    }
    int32_t children[4] = {icon, title, 0, 0};
    uint32_t childCount = 2;
    if (row->detail && row->detail[0]) {
        Change detailChanges[] = {{P(text), kString, row->detail, 0},
                                  {P(fontSize), kNumber, NULL, 14},
                                  {P(opacity), kNumber, NULL, 0.62},
                                  {P(flex), kDrop, NULL, 0}};
        children[childCount++] = cloneRole(out, kRowTitle, detailChanges, 4);
    }
    if (row->kind == kAstralRowLink) {
        int32_t trailing = cloneRole(out, kRowTrailing, NULL, 0);
        int32_t chevron = cloneRole(out, kRowChevron, NULL, 0);
        putChildren(out, trailing, (int32_t[]){chevron}, 1);
        children[childCount++] = trailing;
    } else if (row->kind == kAstralRowSelected) {
        int32_t trailing = cloneRole(out, kRowTrailing, NULL, 0);
        Change checkChange = {P(uri), kString, "res://ic_mana_checkmark_large_bold", 0};
        int32_t check = cloneRole(out, kRowChevron, &checkChange, 1);
        putChildren(out, trailing, (int32_t[]){check}, 1);
        children[childCount++] = trailing;
    } else if (row->kind == kAstralRowSwitch || row->kind == kAstralRowSwitchLink) {
        int32_t trailing = newNode(out, "view");
        propCount(out, P(accessibilityBusy));
        propString(out, P(align), "stretch");
        propQuad(out, P(margin), 18, 0, 0, 0);
        putChildren(out, trailing, (int32_t[]){buildSwitch(out, row->key, row->on)}, 1);
        children[childCount++] = trailing;
    }
    putChildren(out, rowId, (int32_t[]){content}, 1);
    putChildren(out, content, children, childCount);
    return rowId;
}

static int64_t templateColor(int role, uint16_t wanted, int64_t fallback)
{
    if (!gTemplates[role].bytes) return fallback;
    Cursor cursor = {gTemplates[role].bytes, gTemplates[role].length, 1 + 4, 0};
    skip(&cursor, read16(&cursor));
    uint16_t count = read16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        if (property == wanted && (tag == 3 || tag == 4)) {
            uint32_t low = read32(&cursor);
            return tag == 3 ? (int64_t)(int32_t)low : (int64_t)low;
        }
        skipValue(&cursor, tag, 0);
    }
    return fallback;
}

static const int64_t kTranslucentFill = 0x1FFFFFFF;
static const int64_t kSubtleBorder = 0x1F97979F;
static const int64_t kCriticalText = 0xFFF23F43;

static int32_t buildColoredText(Out *out, const char *text, double size, int32_t weight, double opacity, int32_t lines, int grow,
                                int64_t color)
{
    int32_t node = newNode(out, "text");
    propCount(out, (uint16_t)(6 + (lines > 0) + (grow > 0)));
    propString(out, P(text), text);
    propLong(out, P(color), color);
    propNumber(out, P(fontSize), size);
    propInt(out, P(fontWeight), weight);
    propNumber(out, P(lineHeight), size + 4);
    propNumber(out, P(opacity), opacity);
    if (lines > 0) propInt(out, P(maxLines), lines);
    if (grow > 0) propNumber(out, P(flex), 1);
    return node;
}

static int32_t buildText(Out *out, const char *text, double size, int32_t weight, double opacity, int32_t lines, int grow)
{
    return buildColoredText(out, text, size, weight, opacity, lines, grow, templateColor(kRowTitle, P(color), 0xFFFBFBFB));
}

static int32_t buildIcon(Out *out, const char *source, double size)
{
    Change changes[] = {
        {P(uri), kString, source, 0}, {P(width), kNumber, NULL, size}, {P(height), kNumber, NULL, size}, {P(margin), kDrop, NULL, 0}};
    return cloneRole(out, kRowIcon, changes, 4);
}

static int32_t buildRoundButton(Out *out, const char *key, const char *icon, double size)
{
    char identifier[128];
    snprintf(identifier, sizeof(identifier), "astral.row.%s", key);
    int32_t button = newNode(out, "view");
    propCount(out, P(accessibilityKeepsChildren));
    propString(out, P(accessibilityId), identifier);
    propString(out, P(align), "center");
    propString(out, P(justify), "center");
    propLong(out, P(background), kTranslucentFill);
    propNumber(out, P(width), size);
    propNumber(out, P(height), size);
    propNumber(out, P(radius), size / 2);
    propEvents(out, P(on), "press");
    propString(out, P(role), "button");
    propNumber(out, P(shrink), 0);
    putChildren(out, button, (int32_t[]){buildIcon(out, icon, size * 0.55)}, 1);
    return button;
}

static int32_t buildChip(Out *out, const AstralChipSpec *chip)
{
    char identifier[128];
    snprintf(identifier, sizeof(identifier), "astral.row.%s", chip->key);
    int32_t node = newNode(out, "view");
    propCount(out, P(accessibilityId));
    propString(out, P(accessibilityId), identifier);
    propString(out, P(align), "center");
    propString(out, P(direction), "row");
    if (chip->selected)
        propLong(out, P(background), kSwitchOn);
    else
        propLong(out, P(background), kTranslucentFill);
    propQuad(out, P(padding), 14, 7, 14, 7);
    propNumber(out, P(radius), 16);
    propEvents(out, P(on), "press");
    propString(out, P(role), "button");
    propNumber(out, P(shrink), 0);
    putChildren(out, node, (int32_t[]){buildText(out, chip->title, 14, 600, 1, 1, 0)}, 1);
    return node;
}

static int32_t buildToolbar(Out *out, const AstralToolbarSpec *toolbar)
{
    int32_t children[12];
    uint32_t count = 0;
    for (size_t index = 0; index < toolbar->chipCount && count < 8; index++) children[count++] = buildChip(out, &toolbar->chips[index]);
    int32_t spacer = newNode(out, "view");
    propCount(out, P(accessibilityActions));
    propNumber(out, P(flex), 1);
    children[count++] = spacer;
    if (toolbar->buttonKey) children[count++] = buildRoundButton(out, toolbar->buttonKey, toolbar->buttonIcon, 36);
    int32_t row = newNode(out, "view");
    propCount(out, P(accessibilityExpanded));
    propString(out, P(direction), "row");
    propString(out, P(align), "center");
    propNumber(out, P(gap), 8);
    propString(out, P(width), "fill");
    propQuad(out, P(margin), 0, 0, 0, 4);
    putChildren(out, row, children, count);
    return row;
}

static int32_t buildVersionTag(Out *out, const char *text)
{
    int32_t tag = newNode(out, "view");
    propCount(out, P(accessibilityDisabled));
    propLong(out, P(background), kTranslucentFill);
    propQuad(out, P(padding), 6, 2, 6, 2);
    propNumber(out, P(radius), 6);
    propNumber(out, P(shrink), 0);
    putChildren(out, tag, (int32_t[]){buildText(out, text, 11, 600, 0.62, 1, 0)}, 1);
    return tag;
}

static int32_t buildAvatar(Out *out, const char *source, int first)
{
    int32_t avatar = newNode(out, "image");
    propCount(out, P(accessibilityHidden));
    propString(out, P(uri), source);
    propNumber(out, P(width), 24);
    propNumber(out, P(height), 24);
    propNumber(out, P(radius), 12);
    propString(out, P(scale), "fill");
    propQuad(out, P(margin), first ? 0 : -6, 0, 0, 0);
    propNumber(out, P(shrink), 0);
    return avatar;
}

static int32_t buildAuthors(Out *out, const AstralCardSpec *card)
{
    int32_t children[2];
    uint32_t count = 0;
    if (card->avatarCount) {
        int32_t avatars[3];
        size_t avatarCount = card->avatarCount > 3 ? 3 : card->avatarCount;
        for (size_t index = 0; index < avatarCount; index++) avatars[index] = buildAvatar(out, card->avatars[index], index == 0);
        int32_t pile = newNode(out, "view");
        propCount(out, P(accessibilityChecked));
        propString(out, P(direction), "row");
        propString(out, P(align), "center");
        propNumber(out, P(shrink), 0);
        putChildren(out, pile, avatars, (uint32_t)avatarCount);
        children[count++] = pile;
    }
    int32_t names = buildText(out, card->authors, 12, 500, 0.62, 1, 0);
    children[count++] = names;
    int32_t row = newNode(out, "view");
    propCount(out, P(accessibilityChecked));
    propString(out, P(direction), "row");
    propString(out, P(align), "center");
    propNumber(out, P(gap), 6);
    putChildren(out, row, children, count);
    return row;
}

static int32_t buildCard(Out *out, const AstralCardSpec *card)
{
    char settingsKey[128], overflowKey[128];
    snprintf(settingsKey, sizeof(settingsKey), "settings.%s", card->key);
    snprintf(overflowKey, sizeof(overflowKey), "more.%s", card->key);
    int32_t tile = newNode(out, "view");
    propCount(out, P(accessibilityHidden));
    propString(out, P(align), "center");
    propString(out, P(justify), "center");
    propLong(out, P(background), kTranslucentFill);
    propNumber(out, P(width), 40);
    propNumber(out, P(height), 40);
    propNumber(out, P(radius), 12);
    propNumber(out, P(shrink), 0);
    putChildren(out, tile, (int32_t[]){buildIcon(out, card->icon ? card->icon : "res://astral_plug", 22)}, 1);

    int32_t headingChildren[2];
    uint32_t headingCount = 0;
    headingChildren[headingCount++] = buildText(out, card->title, 16, 600, 1, 1, 0);
    if (card->authors && card->authors[0]) headingChildren[headingCount++] = buildAuthors(out, card);
    int32_t heading = newNode(out, "view");
    propCount(out, P(accessibilityDisabled));
    propString(out, P(direction), "column");
    propNumber(out, P(flex), 1);
    propNumber(out, P(gap), 2);
    propNumber(out, P(minWidth), 0);
    putChildren(out, heading, headingChildren, headingCount);

    int32_t actionChildren[3];
    uint32_t actionCount = 0;
    if (card->hasSettings) actionChildren[actionCount++] = buildRoundButton(out, settingsKey, "res://ic_mana_settings", 32);
    actionChildren[actionCount++] = buildRoundButton(out, overflowKey, "res://ic_mana_more_horizontal", 32);
    actionChildren[actionCount++] = buildSwitch(out, card->key, card->on);
    int32_t actions = newNode(out, "view");
    propCount(out, P(accessibilityDisabled));
    propString(out, P(direction), "row");
    propString(out, P(align), "center");
    propNumber(out, P(gap), 8);
    propNumber(out, P(shrink), 0);
    putChildren(out, actions, actionChildren, actionCount);

    int32_t top = newNode(out, "view");
    propCount(out, P(accessibilityChecked));
    propString(out, P(direction), "row");
    propString(out, P(align), "start");
    propNumber(out, P(gap), 12);
    putChildren(out, top, (int32_t[]){tile, heading, actions}, 3);

    int32_t bottomChildren[2];
    uint32_t bottomCount = 0;
    if (card->description && card->description[0]) {
        bottomChildren[bottomCount++] = buildText(out, card->description, 14, 400, 1, 0, 1);
    } else {
        int32_t spacer = newNode(out, "view");
        propCount(out, P(accessibilityActions));
        propNumber(out, P(flex), 1);
        bottomChildren[bottomCount++] = spacer;
    }
    if (card->version && card->version[0]) bottomChildren[bottomCount++] = buildVersionTag(out, card->version);
    int32_t bottom = newNode(out, "view");
    propCount(out, P(accessibilityChecked));
    propString(out, P(direction), "row");
    propString(out, P(align), "end");
    propNumber(out, P(gap), 12);
    putChildren(out, bottom, bottomChildren, bottomCount);

    int32_t cardChildren[3] = {top, bottom, 0};
    uint32_t cardCount = 2;
    if (card->failed && card->error && card->error[0]) {
        cardChildren[cardCount++] = buildColoredText(out, card->error, 12, 500, 1, 3, 0, kCriticalText);
    }
    int32_t node = newNode(out, "view");
    propCount(out, P(accessibilityId));
    propString(out, P(direction), "column");
    propNumber(out, P(gap), 6);
    propQuad(out, P(padding), 16, 16, 16, 16);
    propLong(out, P(background), templateColor(kRow, P(background), 0x1497979F));
    propNumber(out, P(radius), 16);
    propLong(out, P(borderColor), kSubtleBorder);
    propNumber(out, P(borderWidth), 1);
    propNumber(out, P(opacity), card->failed ? 0.5 : 1);
    propString(out, P(width), "fill");
    putChildren(out, node, cardChildren, cardCount);
    return node;
}

static int32_t buildCardList(Out *out, const AstralPageSpec *page)
{
    int32_t children[66];
    uint32_t count = 0;
    if (page->toolbar) children[count++] = buildToolbar(out, page->toolbar);
    for (size_t index = 0; index < page->cardCount && count < 65; index++) children[count++] = buildCard(out, &page->cards[index]);
    int32_t list = newNode(out, "view");
    propCount(out, P(accessibilityDisabled));
    propString(out, P(direction), "column");
    propNumber(out, P(gap), 12);
    propString(out, P(width), "fill");
    propQuad(out, P(margin), 0, 4, 0, 8);
    putChildren(out, list, children, count);
    return list;
}

static void buildSections(Out *out, const AstralPageSpec *page, int32_t *sections, uint32_t *sectionCount)
{
    *sectionCount = 0;
    if (page->toolbar || page->cardCount) sections[(*sectionCount)++] = buildCardList(out, page);
    for (size_t index = 0; index < page->count && *sectionCount < 16; index++) {
        const AstralSectionSpec *section = &page->sections[index];
        int32_t group = cloneRole(out, kGroup, NULL, 0);
        Change headerChange = {P(text), kString, section->title, 0};
        int32_t header = cloneRole(out, kGroupHeader, &headerChange, 1);
        int32_t card = cloneRole(out, kCard, NULL, 0);
        int32_t cardChildren[64];
        uint32_t cardCount = 0;
        for (size_t row = 0; row < section->count && cardCount < 62; row++) {
            if (row) {
                int32_t separator = cloneRole(out, kSeparator, NULL, 0);
                int32_t line = cloneRole(out, kSeparatorLine, NULL, 0);
                putChildren(out, separator, (int32_t[]){line}, 1);
                cardChildren[cardCount++] = separator;
            }
            cardChildren[cardCount++] = buildSpecRow(out, &section->rows[row], row == 0, row == section->count - 1);
        }
        putChildren(out, group, (int32_t[]){header, card}, 2);
        putChildren(out, card, cardChildren, cardCount);
        sections[(*sectionCount)++] = group;
    }
}

int astral_page_push(const AstralPageSpec *page, AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    Navigator *navigator = navigatorWithId(gSettingsNavigator);
    if (!gTemplatesReady || !navigator || gDepth >= kMaxPages || navigator->count == 0 || navigator->count >= kMaxNavigatorChildren)
        return 0;
    Out out = {0};
    gPage = &gPages[gDepth];
    gPage->count = 0;
    gPage->firstId = kFirstPageId + (int32_t)gDepth * kPageIdStride;
    put8(&out, 1);
    int32_t root = cloneRole(&out, kRoot, NULL, 0);
    int32_t phone = cloneRole(&out, kPhone, NULL, 0);
    int32_t column = cloneRole(&out, kColumn, NULL, 0);
    int32_t container = cloneRole(&out, kContainer, NULL, 0);
    int32_t spacer = cloneRole(&out, kSpacer, NULL, 0);
    int32_t hairline = cloneRole(&out, kHairline, NULL, 0);
    int32_t scroll = cloneRole(&out, kScroll, NULL, 0);
    int32_t list = cloneRole(&out, kList, NULL, 0);
    uint32_t contentStart = gPage->count;
    int32_t sections[16];
    uint32_t sectionCount = 0;
    buildSections(&out, page, sections, &sectionCount);
    uint32_t contentEnd = gPage->count;
    int32_t headerBar = cloneRole(&out, kHeaderBar, NULL, 0);
    int32_t left = cloneRole(&out, kHeaderLeft, NULL, 0);
    int32_t leftInner = cloneRole(&out, kHeaderLeftInner, NULL, 0);
    int32_t leftPad = cloneRole(&out, kHeaderLeftPad, NULL, 0);
    Change backChange = {P(accessibilityId), kString, "astral.back", 0};
    int32_t back = cloneRole(&out, kBack, &backChange, 1);
    int32_t backStack = cloneRole(&out, kBackStack, NULL, 0);
    int32_t backStackInner = cloneRole(&out, kBackStackInner, NULL, 0);
    Change backIconChange = {P(uri), kString, "res://ic_mana_arrow_large_left", 0};
    int32_t backIcon = cloneRole(&out, kBackIcon, &backIconChange, 1);
    int32_t titleWrap = cloneRole(&out, kTitleWrap, NULL, 0);
    int32_t titleInner = cloneRole(&out, kTitleInner, NULL, 0);
    Change titleChanges[] = {{P(accessibilityId), kString, "settings-astral-title", 0}, {P(text), kString, page->title, 0}};
    int32_t title = cloneRole(&out, kTitle, titleChanges, 2);

    putChildren(&out, root, (int32_t[]){phone}, 1);
    putChildren(&out, phone, (int32_t[]){column}, 1);
    putChildren(&out, column, (int32_t[]){container}, 1);
    putChildren(&out, container, (int32_t[]){spacer, hairline, scroll, headerBar}, 4);
    putChildren(&out, scroll, (int32_t[]){list}, 1);
    putChildren(&out, list, sections, sectionCount);
    putChildren(&out, headerBar, (int32_t[]){left, titleWrap}, 2);
    putChildren(&out, left, (int32_t[]){leftInner}, 1);
    putChildren(&out, leftInner, (int32_t[]){leftPad}, 1);
    putChildren(&out, leftPad, (int32_t[]){back}, 1);
    putChildren(&out, back, (int32_t[]){backStack}, 1);
    putChildren(&out, backStack, (int32_t[]){backStackInner}, 1);
    putChildren(&out, backStackInner, (int32_t[]){backIcon}, 1);
    putChildren(&out, titleWrap, (int32_t[]){titleInner}, 1);
    putChildren(&out, titleInner, (int32_t[]){title}, 1);

    gPage->root = root;
    gPage->list = list;
    gPage->contentStart = contentStart;
    gPage->contentEnd = contentEnd;
    gPage->previousCount = navigator->count;
    memcpy(gPage->previousChildren, navigator->children, sizeof(int32_t) * navigator->count);
    gPage->previousIndex = navigator->index;
    int32_t stack[kMaxNavigatorChildren];
    memcpy(stack, navigator->children, sizeof(int32_t) * navigator->count);
    stack[navigator->count] = root;
    putChildren(&out, navigator->id, stack, navigator->count + 1);
    put8(&out, 2);
    put32(&out, (uint32_t)navigator->id);
    put16(&out, 1);
    put16(&out, 252);
    put8(&out, 3);
    put32(&out, navigator->count);
    if (out.failed || gPage->count >= kMaxPageNodes) {
        free(out.bytes);
        gPage->count = 0;
        return 0;
    }
    navigator->index = (int32_t)navigator->count;
    navigator->children[navigator->count++] = root;
    gDepth++;
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

int astral_page_refresh(const AstralPageSpec *page, AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    if (!gDepth) return 0;
    gPage = &gPages[gDepth - 1];
    int32_t stale[kMaxPageNodes];
    uint32_t staleCount = gPage->contentEnd - gPage->contentStart;
    memcpy(stale, gPage->ids + gPage->contentStart, sizeof(int32_t) * staleCount);
    int32_t kept[kMaxPageNodes];
    uint32_t keptCount = 0;
    for (uint32_t index = 0; index < gPage->count; index++) {
        if (index < gPage->contentStart || index >= gPage->contentEnd) kept[keptCount++] = gPage->ids[index];
    }
    int32_t savedIds[kMaxPageNodes];
    uint32_t savedCount = gPage->count;
    memcpy(savedIds, gPage->ids, sizeof(int32_t) * savedCount);
    int32_t highest = 0;
    for (uint32_t index = 0; index < savedCount; index++)
        if (savedIds[index] > highest) highest = savedIds[index];
    memcpy(gPage->ids, kept, sizeof(int32_t) * keptCount);
    gPage->count = keptCount;
    gPage->nextId = highest + 1;

    Out out = {0};
    put8(&out, 1);
    uint32_t contentStart = gPage->count;
    int32_t sections[16];
    uint32_t sectionCount = 0;
    buildSections(&out, page, sections, &sectionCount);
    putChildren(&out, gPage->list, sections, sectionCount);
    put8(&out, 4);
    put32(&out, staleCount);
    for (uint32_t index = 0; index < staleCount; index++) put32(&out, (uint32_t)stale[index]);
    if (out.failed || gPage->count >= kMaxPageNodes) {
        free(out.bytes);
        memcpy(gPage->ids, savedIds, sizeof(int32_t) * savedCount);
        gPage->count = savedCount;
        return 0;
    }
    gPage->contentStart = contentStart;
    gPage->contentEnd = gPage->count;
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

int astral_page_pop(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    Navigator *navigator = navigatorWithId(gSettingsNavigator);
    if (!gDepth || !navigator) return 0;
    gPage = &gPages[gDepth - 1];
    Out out = {0};
    put8(&out, 1);
    put8(&out, 2);
    put32(&out, (uint32_t)navigator->id);
    put16(&out, 1);
    put16(&out, 252);
    put8(&out, 3);
    put32(&out, (uint32_t)gPage->previousIndex);
    putChildren(&out, navigator->id, gPage->previousChildren, gPage->previousCount);
    putPageRemoval(&out, gPage);
    if (out.failed) {
        free(out.bytes);
        return 0;
    }
    navigator->count = gPage->previousCount;
    memcpy(navigator->children, gPage->previousChildren, sizeof(int32_t) * gPage->previousCount);
    navigator->index = gPage->previousIndex;
    gDepth--;
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

int astral_page_depth(void)
{
    return (int)gDepth;
}

enum { kTrackCapacity = 32768, kMaxTags = 4, kMaxDecorated = 4096 };

static const int32_t kFirstTagId = 0x7b000000;

typedef struct {
    int32_t id;
    uint8_t kind;
    uint64_t value;
} Tracked;

enum { kTrackNone, kTrackAuthor, kTrackAvatar, kTrackAvatarImage, kTrackChannel, kTrackTimestamp };

static Tracked gTracked[kTrackCapacity];
static uint64_t gMessageAuthors[kTrackCapacity][2];
static AstralDecorator gDecorator;
static uint64_t gCurrentChannel;
static int32_t gNextTagId = kFirstTagId;

typedef struct {
    int32_t parent;
    int32_t tags[kMaxTags * 2];
    uint32_t count;
} Decorated;

static Decorated gDecorated[kMaxDecorated];

void astral_wire_set_decorator(AstralDecorator decorator)
{
    gDecorator = decorator;
}

static AstralWireStats gStats;

AstralWireStats astral_wire_stats(void)
{
    return gStats;
}

uint64_t astral_wire_current_channel(void)
{
    return gCurrentChannel;
}

static Tracked *trackSlot(int32_t id, int create)
{
    uint32_t index = ((uint32_t)id * 2654435761u) % kTrackCapacity;
    for (uint32_t probe = 0; probe < kTrackCapacity; probe++) {
        Tracked *slot = &gTracked[(index + probe) % kTrackCapacity];
        if (slot->id == id) return slot;
        if (slot->id == 0) {
            if (!create) return NULL;
            slot->id = id;
            return slot;
        }
    }
    return NULL;
}

static void forget(int32_t id)
{
    Tracked *slot = trackSlot(id, 0);
    if (slot) slot->kind = kTrackNone;
}

static void rememberAuthor(uint64_t message, uint64_t user)
{
    uint32_t index = (uint32_t)(message % kTrackCapacity);
    gMessageAuthors[index][0] = message;
    gMessageAuthors[index][1] = user;
}

static uint64_t authorOf(uint64_t message)
{
    uint32_t index = (uint32_t)(message % kTrackCapacity);
    return gMessageAuthors[index][0] == message ? gMessageAuthors[index][1] : 0;
}

static uint64_t parseId(const uint8_t *text, size_t length)
{
    uint64_t value = 0;
    size_t digits = 0;
    for (size_t index = 0; index < length && text[index] >= '0' && text[index] <= '9'; index++, digits++) {
        value = value * 10 + (uint64_t)(text[index] - '0');
    }
    return digits >= 15 ? value : 0;
}

static uint64_t avatarUser(const uint8_t *text, size_t length)
{
    static const char *markers[] = {"/avatars/", "/users/"};
    for (size_t marker = 0; marker < 2; marker++) {
        size_t markerLength = strlen(markers[marker]);
        for (size_t index = 0; index + markerLength < length; index++) {
            if (memcmp(text + index, markers[marker], markerLength) == 0) {
                uint64_t user = parseId(text + index + markerLength, length - index - markerLength);
                if (user) return user;
            }
        }
    }
    return 0;
}

static int prefixed(const uint8_t *text, size_t length, const char *prefix, uint64_t *suffix)
{
    size_t prefixLength = strlen(prefix);
    if (length <= prefixLength || memcmp(text, prefix, prefixLength) != 0) return 0;
    *suffix = parseId(text + prefixLength, length - prefixLength);
    return *suffix != 0;
}

static void observeProperties(const uint8_t *bytes, const Span *span)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) skip(&cursor, read16(&cursor));
    uint16_t count = read16(&cursor);
    int selected = -1;
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        if (tag == 6 && (property == P(accessibilityId) || property == P(uri))) {
            uint32_t length = read32(&cursor);
            if (!need(&cursor, length)) return;
            const uint8_t *text = bytes + cursor.offset;
            cursor.offset += length;
            uint64_t value = 0;
            int kind = kTrackNone;
            if (property == P(accessibilityId) && prefixed(text, length, "message.author:", &value)) {
                kind = kTrackAuthor;
            } else if (property == P(accessibilityId) && prefixed(text, length, "message.avatar:", &value)) {
                kind = kTrackAvatar;
            } else if (property == P(accessibilityId) && prefixed(text, length, "message.timestamp:", &value)) {
                kind = kTrackTimestamp;
            } else if (property == P(accessibilityId) && prefixed(text, length, "navigation.channel:", &value)) {
                kind = kTrackChannel;
            } else if (property == P(uri) && (value = avatarUser(text, length))) {
                kind = kTrackAvatarImage;
            }
            Tracked *slot = kind == kTrackNone ? NULL : trackSlot(span->id, 1);
            if (slot) *slot = (Tracked){span->id, kind, value};
            continue;
        }
        if (property == P(accessibilitySelected) && (tag == 1 || tag == 2)) {
            selected = tag == 2;
            continue;
        }
        skipValue(&cursor, tag, 0);
    }
    Tracked *slot = selected == 1 ? trackSlot(span->id, 0) : NULL;
    if (slot && slot->kind == kTrackChannel) gCurrentChannel = slot->value;
}

static void propagateAvatars(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (int pass = 0; pass < 3; pass++) {
        for (size_t index = 0; index < count; index++) {
            const Span *span = &spans[index];
            if (span->op != 3 || span->childCount == 0 || span->childCount > 4) continue;
            for (uint32_t position = 0; position < span->childCount; position++) {
                Tracked *child = trackSlot(child_at(bytes, span, position), 0);
                if (!child || child->kind != kTrackAvatarImage) continue;
                Tracked *holder = trackSlot(span->id, 1);
                if (!holder) break;
                if (holder->kind == kTrackAvatar) {
                    rememberAuthor(holder->value, child->value);
                } else if (holder->kind == kTrackNone || holder->kind == kTrackAvatarImage) {
                    *holder = (Tracked){span->id, kTrackAvatarImage, child->value};
                }
                break;
            }
        }
    }
}

static Decorated *decoratedSlot(int32_t parent, int create)
{
    uint32_t index = ((uint32_t)parent * 2654435761u) % kMaxDecorated;
    for (uint32_t probe = 0; probe < kMaxDecorated; probe++) {
        Decorated *slot = &gDecorated[(index + probe) % kMaxDecorated];
        if (slot->parent == parent) return slot;
        if (slot->parent == 0) {
            if (!create) return NULL;
            slot->parent = parent;
            slot->count = 0;
            return slot;
        }
    }
    return NULL;
}

static void removeTags(Out *out, Decorated *slot)
{
    if (!slot || !slot->count) return;
    put8(out, 4);
    put32(out, slot->count);
    for (uint32_t index = 0; index < slot->count; index++) put32(out, (uint32_t)slot->tags[index]);
    slot->count = 0;
}

static int32_t tagId(void)
{
    int32_t id = gNextTagId++;
    if (gNextTagId >= kFirstTagId + 0x00f00000) gNextTagId = kFirstTagId;
    return id;
}

static void propGradient(Out *out, uint16_t property, const uint32_t *colors, uint32_t count)
{
    put16(out, property);
    put8(out, 8);
    put32(out, 3);
    put16(out, P(angle));
    put8(out, 5);
    double angle = 90;
    put(out, &angle, 8);
    put16(out, P(colors));
    put8(out, 7);
    put32(out, count);
    for (uint32_t index = 0; index < count; index++) {
        put8(out, 4);
        int64_t color = colors[index];
        put(out, &color, 8);
    }
    put16(out, P(positions));
    put8(out, 7);
    put32(out, count);
    for (uint32_t index = 0; index < count; index++) {
        put8(out, 5);
        double position = count > 1 ? (double)index / (count - 1) : 0;
        put(out, &position, 8);
    }
}

static int32_t buildTag(Out *out, const AstralTag *tag, Decorated *slot)
{
    int originalPoster = tag->shape == kAstralTagOriginalPoster;
    int gradient = tag->gradientCount >= 2;
    int32_t pill = tagId();
    put8(out, 1);
    put32(out, (uint32_t)pill);
    put16(out, 4);
    put(out, "view", 4);
    put16(out, 6);
    propString(out, P(align), "center");
    if (gradient)
        propGradient(out, P(backgroundGradient), tag->gradient, tag->gradientCount > 3 ? 3 : tag->gradientCount);
    else
        propLong(out, P(background), (int64_t)tag->background);
    propString(out, P(direction), "row");
    propNumber(out, P(height), originalPoster ? 16 : 18);
    propQuad(out, P(padding), originalPoster ? 5 : 4, 0, originalPoster ? 5 : 4, 0);
    propNumber(out, P(radius), originalPoster ? 8 : 4);
    int32_t label = tagId();
    put8(out, 1);
    put32(out, (uint32_t)label);
    put16(out, 4);
    put(out, "text", 4);
    put16(out, originalPoster ? 8 : 7);
    propLong(out, P(color), (int64_t)tag->foreground);
    propNumber(out, P(fontSize), originalPoster ? 10 : 12);
    propInt(out, P(fontWeight), originalPoster ? 700 : 600);
    propNumber(out, P(lineHeight), originalPoster ? 13 : 16);
    propInt(out, P(maxLines), 1);
    propString(out, P(text), tag->text);
    propBool(out, P(notoFallback), 1);
    if (originalPoster) {
        propString(out, P(textTransform), "uppercase");
    }
    putChildren(out, pill, (int32_t[]){label}, 1);
    slot->tags[slot->count++] = pill;
    slot->tags[slot->count++] = label;
    return pill;
}

static void decorate(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gDecorator) return;
    propagateAvatars(bytes, spans, count);
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 3 || span->childCount == 0 || span->childCount > 64) continue;
        int32_t matched = 0;
        uint64_t message = 0;
        for (uint32_t position = 0; position < span->childCount; position++) {
            Tracked *child = trackSlot(child_at(bytes, span, position), 0);
            if (child && child->kind == kTrackAuthor) {
                matched = child->id;
                message = child->value;
                break;
            }
        }
        if (!matched) continue;
        AstralDecorationContext context = {"message.author:", message, authorOf(message), gCurrentChannel};
        AstralTag tags[kMaxTags];
        memset(tags, 0, sizeof(tags));
        size_t tagCount = gDecorator(&context, tags, kMaxTags);
        gStats.authors++;
        if (context.user_id) gStats.with_user++;
        if (context.channel_id) gStats.with_channel++;
        if (tagCount) gStats.tagged++;
        Decorated *slot = decoratedSlot(span->id, tagCount > 0);
        if (!tagCount && (!slot || !slot->count)) continue;
        if (!*changed) put(out, bytes, length);
        *changed = 1;
        removeTags(out, slot);
        if (!tagCount) continue;
        int32_t children[72];
        uint32_t used = 0;
        int32_t created[kMaxTags];
        for (size_t tag = 0; tag < tagCount && tag < kMaxTags; tag++) created[tag] = buildTag(out, &tags[tag], slot);
        uint32_t insertAt = span->childCount;
        for (uint32_t position = 0; position < span->childCount; position++) {
            Tracked *child = trackSlot(child_at(bytes, span, position), 0);
            if (child && child->kind == kTrackTimestamp) {
                insertAt = position;
                break;
            }
        }
        for (uint32_t position = 0; position <= span->childCount && used < 68; position++) {
            if (position == insertAt) {
                for (size_t tag = 0; tag < tagCount && tag < kMaxTags; tag++) children[used++] = created[tag];
            }
            if (position < span->childCount) children[used++] = child_at(bytes, span, position);
        }
        putChildren(out, span->id, children, used);
    }
}

static void observeRemovals(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 4) continue;
        for (uint32_t position = 0; position < span->childCount; position++) {
            int32_t id = child_at(bytes, span, position);
            forget(id);
            Decorated *slot = decoratedSlot(id, 0);
            if (slot && slot->count) {
                if (!*changed) put(out, bytes, length);
                *changed = 1;
                removeTags(out, slot);
            }
        }
    }
}

static void observe(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++) {
        if (spans[index].op == 1 || spans[index].op == 2) observeProperties(bytes, &spans[index]);
    }
}

enum { kMaxPalette = 64 };

static uint32_t gPaletteFrom[kMaxPalette];
static uint32_t gPaletteTo[kMaxPalette];
static size_t gPaletteCount;

void astral_wire_set_palette(const uint32_t *from, const uint32_t *to, size_t count)
{
    gPaletteCount = count < kMaxPalette ? count : kMaxPalette;
    if (!gPaletteCount) return;
    memcpy(gPaletteFrom, from, sizeof(uint32_t) * gPaletteCount);
    memcpy(gPaletteTo, to, sizeof(uint32_t) * gPaletteCount);
}

static int paletteColor(uint32_t color, uint32_t *mapped)
{
    for (size_t index = 0; index < gPaletteCount; index++) {
        if (gPaletteFrom[index] == color) {
            *mapped = gPaletteTo[index];
            return 1;
        }
    }
    return 0;
}

static double gRadiusScale = 1, gFontScale = 1;
enum { kMaxBackgroundStops = 8 };
static uint32_t gBackgroundStops[kMaxBackgroundStops];
static size_t gBackgroundStopCount;

void astral_wire_set_background_gradient(const uint32_t *colors, size_t count)
{
    gBackgroundStopCount = count > kMaxBackgroundStops ? kMaxBackgroundStops : count;
    for (size_t index = 0; index < gBackgroundStopCount; index++) gBackgroundStops[index] = colors[index];
}

static uint32_t backgroundStop(uint32_t index, uint32_t count)
{
    if (gBackgroundStopCount == 1 || count <= 1) return gBackgroundStops[0];
    size_t position = (size_t)index * (gBackgroundStopCount - 1) / (count - 1);
    return gBackgroundStops[position];
}

static int patchGradientStops(uint8_t *bytes, Cursor *cursor, int replaceAll)
{
    int patched = 0;
    uint32_t entries = read32(cursor);
    for (uint32_t entry = 0; entry < entries && !cursor->failed; entry++) {
        uint16_t key = read16(cursor);
        uint8_t tag = read8(cursor);
        if (key != P(colors) || tag != 7) {
            skipValue(cursor, tag, 1);
            continue;
        }
        uint32_t count = read32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            uint8_t stopTag = read8(cursor);
            size_t at = cursor->offset;
            skipValue(cursor, stopTag, 2);
            if (cursor->failed || (stopTag != 3 && stopTag != 4)) continue;
            uint32_t color, mapped;
            memcpy(&color, bytes + at, 4);
            if (replaceAll)
                mapped = backgroundStop(index, count);
            else if (!paletteColor(color, &mapped))
                continue;
            memcpy(bytes + at, &mapped, 4);
            if (stopTag == 4) memset(bytes + at + 4, 0, 4);
            patched = 1;
        }
    }
    return patched;
}

void astral_wire_set_scales(double radius, double font)
{
    gRadiusScale = radius > 0 ? radius : 1;
    gFontScale = font > 0 ? font : 1;
}

static int scaledProperty(uint16_t property, double *scale)
{
    if (property == P(radius) || property == P(radiusBottomLeft) || property == P(radiusBottomRight) || property == P(radiusTopLeft) ||
        property == P(radiusTopRight)) {
        *scale = gRadiusScale;
        return gRadiusScale != 1;
    }
    if (property == P(fontSize) || property == P(lineHeight)) {
        *scale = gFontScale;
        return gFontScale != 1;
    }
    return 0;
}

static int colorProperty(uint16_t property)
{
    return property == P(background) || property == P(color) || property == P(borderColor) || property == P(activeBackground) ||
           property == P(activeBorderColor) || property == P(focusBorderColor) || property == P(placeholderColor) || property == P(tint);
}

static int patchColors(uint8_t *bytes, const Span *spans, size_t count)
{
    int patched = 0;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2) continue;
        Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
        if (span->op == 1) skip(&cursor, read16(&cursor));
        uint16_t properties = read16(&cursor);
        for (uint16_t entry = 0; entry < properties && !cursor.failed; entry++) {
            uint16_t property = read16(&cursor);
            uint8_t tag = read8(&cursor);
            size_t at = cursor.offset;
            skipValue(&cursor, tag, 0);
            double scale = 1;
            if (!cursor.failed && tag == 5 && scaledProperty(property, &scale)) {
                double value;
                memcpy(&value, bytes + at, 8);
                if (value > 0 && value < 9000) {
                    value *= scale;
                    memcpy(bytes + at, &value, 8);
                    patched = 1;
                }
                continue;
            }
            if (!cursor.failed && property == P(backgroundGradient) && tag == 8) {
                Cursor gradient = {bytes, cursor.offset, at, 0};
                int replaceAll = gBackgroundStopCount > 0 && span->id > 0 && span->id < kFirstId;
                if ((replaceAll || gPaletteCount) && patchGradientStops(bytes, &gradient, replaceAll)) patched = 1;
                continue;
            }
            if (cursor.failed || !colorProperty(property) || (tag != 3 && tag != 4)) continue;
            uint32_t color, mapped;
            memcpy(&color, bytes + at, 4);
            if (!paletteColor(color, &mapped)) continue;
            memcpy(bytes + at, &mapped, 4);
            if (tag == 4) memset(bytes + at + 4, 0, 4);
            patched = 1;
        }
    }
    return patched;
}

enum { kMaxLabelEdits = 512, kMaxLabelNodes = 2048, kLabelCapacity = 256 };

typedef struct {
    size_t offset;
    uint32_t oldLength;
    char text[kLabelCapacity];
    uint32_t newLength;
} LabelEdit;

typedef struct {
    int32_t id;
    int32_t parent;
} LabelLink;

static AstralRelabeler gRelabeler;

void astral_wire_set_relabeler(AstralRelabeler relabeler)
{
    gRelabeler = relabeler;
}

static int numberAt(const uint8_t *bytes, const Span *span, uint16_t wanted, double *value)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) skip(&cursor, read16(&cursor));
    uint16_t count = read16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        if (property == wanted && tag == 5) {
            if (!need(&cursor, 8)) return 0;
            memcpy(value, bytes + cursor.offset, 8);
            return 1;
        }
        skipValue(&cursor, tag, 0);
    }
    return 0;
}

static int stringAt(const uint8_t *bytes, const Span *span, uint16_t wanted, size_t *offset, uint32_t *length)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) skip(&cursor, read16(&cursor));
    uint16_t count = read16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        if (property == wanted && tag == 6) {
            size_t at = cursor.offset;
            uint32_t size = read32(&cursor);
            if (!need(&cursor, size)) return 0;
            *offset = at;
            *length = size;
            return 1;
        }
        skipValue(&cursor, tag, 0);
    }
    return 0;
}

static int firstSpanText(const uint8_t *bytes, const Span *span, size_t *offset, uint32_t *length)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) skip(&cursor, read16(&cursor));
    uint16_t count = read16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = read16(&cursor);
        uint8_t tag = read8(&cursor);
        if (property != P(spans) || tag != 7) {
            skipValue(&cursor, tag, 0);
            continue;
        }
        if (read32(&cursor) == 0 || read8(&cursor) != 8) return 0;
        uint32_t entries = read32(&cursor);
        for (uint32_t entry = 0; entry < entries && !cursor.failed; entry++) {
            uint16_t key = read16(&cursor);
            uint8_t valueTag = read8(&cursor);
            if (key == 499 && valueTag == 6) {
                size_t at = cursor.offset;
                uint32_t size = read32(&cursor);
                if (!need(&cursor, size)) return 0;
                *offset = at;
                *length = size;
                return 1;
            }
            skipValue(&cursor, valueTag, 0);
        }
        return 0;
    }
    return 0;
}

static int spanContains(const uint8_t *bytes, const Span *span, const char *needle)
{
    size_t size = strlen(needle);
    for (size_t at = span->start; at + size <= span->end; at++) {
        if (memcmp(bytes + at, needle, size) == 0) return 1;
    }
    return 0;
}

static int32_t labelParent(const LabelLink *links, size_t count, int32_t id)
{
    for (size_t index = 0; index < count; index++) {
        if (links[index].id == id) return links[index].parent;
    }
    return 0;
}

static int32_t rowOf(const LabelLink *links, size_t linkCount, const int32_t *rows, size_t rowCount, int32_t id)
{
    for (int depth = 0; depth < 24 && id; depth++) {
        for (size_t index = 0; index < rowCount; index++) {
            if (rows[index] == id) return id;
        }
        id = labelParent(links, linkCount, id);
    }
    return 0;
}

static void addLabelEdit(LabelEdit *edits, size_t *count, const uint8_t *bytes, size_t offset, uint32_t length,
                         const AstralLabelContext *context)
{
    if (*count >= kMaxLabelEdits || length >= kLabelCapacity) return;
    char current[kLabelCapacity];
    memcpy(current, bytes + offset + 4, length);
    current[length] = 0;
    LabelEdit *edit = &edits[*count];
    int written = gRelabeler(context, current, length, edit->text, sizeof(edit->text));
    if (written < 0 || (uint32_t)written >= kLabelCapacity) return;
    if ((uint32_t)written == length && memcmp(edit->text, current, length) == 0) return;
    edit->offset = offset;
    edit->oldLength = length;
    edit->newLength = (uint32_t)written;
    (*count)++;
}

static int compareEdits(const void *left, const void *right)
{
    size_t a = ((const LabelEdit *)left)->offset, b = ((const LabelEdit *)right)->offset;
    return a < b ? -1 : a > b;
}

typedef struct {
    const char *prefix;
    AstralLabelKind kind;
} LabelRowKind;

static const LabelRowKind kLabelRows[] = {
    {"navigation.channel:", kAstralLabelChannel},
    {"channel.details.member:", kAstralLabelMember},
};

static const double kProfileNameSize = 20;

static void labelProfile(const uint8_t *bytes, const Span *spans, size_t count, LabelEdit *edits, size_t *editCount)
{
    int profile = 0;
    uint64_t user = 0;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1) continue;
        size_t offset;
        uint32_t size;
        if (stringProperty(bytes, span, P(accessibilityId), "profile-scroll")) profile = 1;
        if (!user && typeIs(bytes, span, "image") && stringAt(bytes, span, P(uri), &offset, &size))
            user = avatarUser(bytes + offset + 4, size);
    }
    if (!profile) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        double size = 0;
        size_t offset;
        uint32_t length;
        if (span->op != 1 || !typeIs(bytes, span, "text") || !numberAt(bytes, span, P(fontSize), &size) || size < kProfileNameSize)
            continue;
        if (!stringAt(bytes, span, P(text), &offset, &length)) continue;
        AstralLabelContext context = {kAstralLabelProfile, 0, gCurrentChannel, user};
        addLabelEdit(edits, editCount, bytes, offset, length, &context);
        return;
    }
}

static void labelRows(const uint8_t *bytes, const Span *spans, size_t count, const LabelLink *links, size_t linkCount, LabelEdit *edits,
                      size_t *editCount)
{
    labelProfile(bytes, spans, count, edits, editCount);
    enum { kMaxRows = 512 };
    int32_t rowIds[kMaxRows];
    uint64_t rowTargets[kMaxRows];
    AstralLabelKind rowKinds[kMaxRows];
    int rowLabelled[kMaxRows];
    size_t rowCount = 0;
    for (size_t index = 0; index < count && rowCount < kMaxRows; index++) {
        const Span *span = &spans[index];
        if (span->op != 1) continue;
        size_t offset;
        uint32_t size;
        if (!stringAt(bytes, span, P(accessibilityId), &offset, &size)) continue;
        if (size == 8 && memcmp(bytes + offset + 4, "composer", 8) == 0 && stringAt(bytes, span, P(placeholder), &offset, &size)) {
            AstralLabelContext context = {kAstralLabelComposer, 0, gCurrentChannel, 0};
            addLabelEdit(edits, editCount, bytes, offset, size, &context);
            continue;
        }
        for (size_t kind = 0; kind < sizeof(kLabelRows) / sizeof(*kLabelRows); kind++) {
            uint64_t target = 0;
            if (!prefixed(bytes + offset + 4, size, kLabelRows[kind].prefix, &target)) continue;
            rowIds[rowCount] = span->id;
            rowTargets[rowCount] = target;
            rowKinds[rowCount] = kLabelRows[kind].kind;
            rowLabelled[rowCount] = 0;
            rowCount++;
            break;
        }
    }
    if (!rowCount) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 || !typeIs(bytes, span, "text")) continue;
        size_t offset;
        uint32_t size;
        if (!stringAt(bytes, span, P(text), &offset, &size)) continue;
        int32_t node = span->id;
        for (int depth = 0; depth < 8 && node; depth++) {
            node = labelParent(links, linkCount, node);
            size_t row = rowCount;
            for (size_t candidate = 0; candidate < rowCount; candidate++) {
                if (rowIds[candidate] == node) row = candidate;
            }
            if (row == rowCount) continue;
            if (!rowLabelled[row]) {
                rowLabelled[row] = 1;
                uint64_t channel = rowKinds[row] == kAstralLabelChannel ? rowTargets[row] : gCurrentChannel;
                AstralLabelContext context = {rowKinds[row], 0, channel, rowTargets[row]};
                addLabelEdit(edits, editCount, bytes, offset, size, &context);
            }
            break;
        }
    }
}

typedef struct {
    size_t edit;
    ptrdiff_t delta;
} ShiftCursor;

static size_t shiftForward(ShiftCursor *cursor, size_t position, const LabelEdit *edits, size_t editCount)
{
    while (cursor->edit < editCount && edits[cursor->edit].offset + 4 + edits[cursor->edit].oldLength <= position) {
        cursor->delta += (ptrdiff_t)edits[cursor->edit].newLength - (ptrdiff_t)edits[cursor->edit].oldLength;
        cursor->edit++;
    }
    return (size_t)((ptrdiff_t)position + cursor->delta);
}

static void shiftSpans(Span *spans, size_t count, const LabelEdit *edits, size_t editCount)
{
    ShiftCursor starts = {0, 0}, ends = {0, 0}, children = {0, 0};
    for (size_t index = 0; index < count; index++) {
        spans[index].start = shiftForward(&starts, spans[index].start, edits, editCount);
        spans[index].end = shiftForward(&ends, spans[index].end, edits, editCount);
        if (spans[index].children) spans[index].children = shiftForward(&children, spans[index].children, edits, editCount);
    }
}

static uint8_t *relabel(const uint8_t *bytes, size_t length, Span *spans, size_t count, size_t *newLength)
{
    if (!gRelabeler) return NULL;
    static LabelLink links[kMaxChildren];
    static int32_t rows[kMaxLabelNodes];
    static int32_t authorNodes[kMaxLabelNodes];
    static uint64_t authorMessages[kMaxLabelNodes];
    static LabelEdit edits[kMaxLabelEdits];
    size_t linkCount = 0, rowCount = 0, authorCount = 0, editCount = 0;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 3) {
            for (uint32_t position = 0; position < span->childCount && linkCount < kMaxChildren; position++) {
                links[linkCount++] = (LabelLink){child_at(bytes, span, position), span->id};
            }
        } else if (span->op == 1) {
            if (stringProperty(bytes, span, P(accessibilityId), "message-row") && rowCount < kMaxLabelNodes) rows[rowCount++] = span->id;
            size_t offset;
            uint32_t size;
            uint64_t message = 0;
            if (stringAt(bytes, span, P(accessibilityId), &offset, &size) &&
                prefixed(bytes + offset + 4, size, "message.author:", &message) && authorCount < kMaxLabelNodes) {
                authorNodes[authorCount] = span->id;
                authorMessages[authorCount++] = message;
            }
        }
    }
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2) continue;
        size_t offset;
        uint32_t size;
        uint64_t message = 0;
        for (size_t author = 0; author < authorCount; author++) {
            if (authorNodes[author] == span->id) message = authorMessages[author];
        }
        Tracked *tracked = message ? NULL : trackSlot(span->id, 0);
        if (!message && tracked && tracked->kind == kTrackAuthor) message = tracked->value;
        if (message && stringAt(bytes, span, P(text), &offset, &size)) {
            AstralLabelContext context = {kAstralLabelAuthor, message, gCurrentChannel, 0};
            addLabelEdit(edits, &editCount, bytes, offset, size, &context);
            continue;
        }
        if (span->op != 1 || !spanContains(bytes, span, "replyPreview") || !firstSpanText(bytes, span, &offset, &size)) continue;
        int32_t row = rowOf(links, linkCount, rows, rowCount, span->id);
        uint64_t replying = 0;
        for (size_t author = 0; author < authorCount && row; author++) {
            if (rowOf(links, linkCount, rows, rowCount, authorNodes[author]) == row) replying = authorMessages[author];
        }
        if (!replying) continue;
        AstralLabelContext context = {kAstralLabelReply, replying, gCurrentChannel, 0};
        addLabelEdit(edits, &editCount, bytes, offset, size, &context);
    }
    labelRows(bytes, spans, count, links, linkCount, edits, &editCount);
    if (!editCount) return NULL;
    qsort(edits, editCount, sizeof(*edits), compareEdits);
    size_t total = length;
    for (size_t index = 0; index < editCount; index++) total = total - edits[index].oldLength + edits[index].newLength;
    uint8_t *result = malloc(total);
    if (!result) return NULL;
    size_t read = 0, written = 0;
    for (size_t index = 0; index < editCount; index++) {
        const LabelEdit *edit = &edits[index];
        memcpy(result + written, bytes + read, edit->offset - read);
        written += edit->offset - read;
        memcpy(result + written, &edit->newLength, 4);
        memcpy(result + written + 4, edit->text, edit->newLength);
        written += 4 + edit->newLength;
        read = edit->offset + 4 + edit->oldLength;
    }
    memcpy(result + written, bytes + read, length - read);
    shiftSpans(spans, count, edits, editCount);
    *newLength = total;
    return result;
}

enum { kWireVersion = 1 };
static int gUnsupportedWireVersion;
static unsigned gRewriteFailures;

unsigned astral_wire_rewrite_failures(void)
{
    return gRewriteFailures;
}

int astral_wire_unsupported_version(void)
{
    return gUnsupportedWireVersion;
}

int astral_settings_rewrite(const uint8_t *bytes, size_t length, AstralWireBuffer *buffer)
{
    static Span spans[kMaxSpans];
    size_t count = 0;
    memset(buffer, 0, sizeof(*buffer));
    if (length && bytes[0] != kWireVersion && !gUnsupportedWireVersion) gUnsupportedWireVersion = bytes[0];
    if (gUnsupportedWireVersion) return 0;
    if (scan(bytes, length, spans, &count) != 0) return 0;

    Out out = {0};
    int changed = 0;
    size_t relabelledLength = 0;
    uint8_t *relabelled = relabel(bytes, length, spans, count, &relabelledLength);
    if (relabelled) {
        bytes = relabelled;
        length = relabelledLength;
        changed = 1;
    }
    uint8_t *themed = NULL;
    if (gPaletteCount || gRadiusScale != 1 || gFontScale != 1 || gBackgroundStopCount) {
        themed = malloc(length);
        if (themed) {
            memcpy(themed, bytes, length);
            if (patchColors(themed, spans, count) || changed) {
                bytes = themed;
                changed = 1;
            }
        }
    }
    if (changed) put(&out, bytes, length);
    length_of_batch = length;
    observe(bytes, spans, count);
    trackNavigator(bytes, spans, count, &out, &changed);
    if (gState.active) {
        for (size_t index = 0; index < count; index++) {
            if (spans[index].op == 4 && removes(bytes, &spans[index], gState.list)) {
                if (!changed) put(&out, bytes, length);
                putRemoval(&out);
                if (gDepth) putAllPagesRemoval(&out);
                gState.active = 0;
                changed = 1;
                break;
            }
        }
    }
    if (gState.active) {
        const Span *list = childList(bytes, spans, count, gState.list);
        if (list) {
            if (!changed) put(&out, bytes, length);
            putListWithAstral(&out, bytes, list);
            changed = 1;
        }
    } else {
        for (size_t index = 0; index < count; index++) {
            if (spans[index].op == 1 && stringProperty(bytes, &spans[index], P(accessibilityId), kAnchor)) {
                Out category = {0};
                captureTemplates(bytes, spans, count, spans[index].id);
                if (createCategory(&category, bytes, spans, count, &spans[index]) && !category.failed) {
                    if (!changed) put(&out, bytes, length);
                    put(&out, category.bytes, category.length);
                    changed = 1;
                } else {
                    gState.active = 0;
                }
                free(category.bytes);
                break;
            }
        }
    }
    decorate(bytes, spans, count, &out, &changed, length);
    observeRemovals(bytes, spans, count, &out, &changed, length);
    free(themed);
    free(relabelled);
    if (!changed || out.failed) {
        free(out.bytes);
        return 0;
    }
    size_t checked = 0;
    if (scan(out.bytes, out.length, spans, &checked) != 0) {
        gRewriteFailures++;
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

void astral_wire_buffer_free(AstralWireBuffer *buffer)
{
    free(buffer->bytes);
    buffer->bytes = NULL;
    buffer->length = 0;
}
