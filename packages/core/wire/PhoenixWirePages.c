#include "PhoenixWireInternal.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char kAnchor[] = "settings.row.account";

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
static size_t gBatchLength;
static size_t gCategoryRowCount = 1;
static char gCategoryTitle[kCategoryText] = "astral";

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
        wirePut8(out, 6);
        wirePut32(out, (uint32_t)length);
        wirePut(out, change->text, length);
    } else if (change->edit == kInteger) {
        wirePut8(out, 3);
        wirePut32(out, (uint32_t)(int32_t)change->number);
    } else {
        wirePut8(out, 5);
        wirePut(out, &change->number, 8);
    }
}

void wireCloneCreation(Out *out, const uint8_t *bytes, const Span *span, int32_t id, const Change *changes, size_t changeCount)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    uint16_t typeLength = wireRead16(&cursor);
    size_t typeStart = cursor.offset;
    wireSkip(&cursor, typeLength);
    uint16_t count = wireRead16(&cursor);
    if (cursor.failed) {
        out->failed = 1;
        return;
    }
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)id);
    wirePut16(out, typeLength);
    wirePut(out, bytes + typeStart, typeLength);
    size_t countAt = out->length;
    wirePut16(out, 0);
    uint16_t written = 0;
    int seen[kMaxProps] = {0};
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        size_t start = cursor.offset;
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        wireSkipValue(&cursor, tag, 0);
        const Change *change = changeFor(changes, changeCount, property);
        if (change) seen[change - changes] = 1;
        if (change && change->edit == kDrop) continue;
        if (!change && tag == 12) continue;
        if (change && change->edit != kKeep) {
            wirePut16(out, property);
            putValue(out, change);
        } else {
            wirePut(out, bytes + start, cursor.offset - start);
        }
        written++;
    }
    for (size_t index = 0; index < changeCount && index < kMaxProps; index++) {
        if (seen[index] || changes[index].edit == kDrop || changes[index].edit == kKeep) continue;
        wirePut16(out, changes[index].property);
        putValue(out, &changes[index]);
        written++;
    }
    if (cursor.failed) out->failed = 1;
    if (!out->failed) memcpy(out->bytes + countAt, &written, 2);
}

static void putListWithAstral(Out *out, const uint8_t *bytes, const Span *list)
{
    int32_t children[kMaxChildren];
    uint32_t count = 0;
    for (uint32_t index = 0; index < list->childCount && count < kMaxChildren - 1; index++) {
        int32_t id = wireChild(bytes, list, index);
        if (id == gState.ids[0]) continue;
        children[count++] = id;
        if (index == 0) children[count++] = gState.ids[0];
    }
    wirePutChildren(out, list->id, children, count);
}

static int removes(const uint8_t *bytes, const Span *span, int32_t id)
{
    for (uint32_t index = 0; index < span->childCount; index++) {
        if (wireChild(bytes, span, index) == id) return 1;
    }
    return 0;
}

static void putRemoval(Out *out)
{
    wirePut8(out, 4);
    wirePut32(out, gState.count);
    for (uint32_t index = 0; index < gState.count; index++) wirePut32(out, (uint32_t)gState.ids[index]);
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
    const Span *span = wireCreation(spans, count, source);
    if (!span || gState.count >= kMaxCategoryNodes) {
        out->failed = 1;
        return 0;
    }
    int32_t id = kFirstId + (int32_t)gState.count;
    gState.ids[gState.count++] = id;
    wireCloneCreation(out, bytes, span, id, changes, changeCount);
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
    wirePutChildren(out, row, (int32_t[]){content}, 1);
    wirePutChildren(out, content, (int32_t[]){icon, title, trailing}, 3);
    wirePutChildren(out, trailing, (int32_t[]){chevron}, 1);
    return row;
}

static int createCategory(Out *out, const uint8_t *bytes, const Span *spans, size_t count, const Span *row)
{
    int32_t card = wireParentOf(bytes, spans, count, row->id);
    int32_t group = card ? wireParentOf(bytes, spans, count, card) : 0;
    int32_t list = group ? wireParentOf(bytes, spans, count, group) : 0;
    const Span *groupChildren = group ? wireChildList(bytes, spans, count, group) : NULL;
    const Span *cardChildren = card ? wireChildList(bytes, spans, count, card) : NULL;
    const Span *rowChildren = wireChildList(bytes, spans, count, row->id);
    if (!list || !groupChildren || groupChildren->childCount < 2 || !rowChildren || rowChildren->childCount < 1 || !cardChildren) return 0;
    int32_t header = wireChild(bytes, groupChildren, 0);
    int32_t content = wireChild(bytes, rowChildren, 0);
    const Span *contentChildren = wireChildList(bytes, spans, count, content);
    if (!contentChildren || contentChildren->childCount < 3) return 0;
    int32_t trailing = wireChild(bytes, contentChildren, 2);
    const Span *trailingChildren = wireChildList(bytes, spans, count, trailing);
    if (!trailingChildren || trailingChildren->childCount < 1) return 0;
    int32_t source[6] = {row->id,
                         content,
                         wireChild(bytes, contentChildren, 0),
                         wireChild(bytes, contentChildren, 1),
                         trailing,
                         wireChild(bytes, trailingChildren, 0)};
    int32_t separator = cardChildren->childCount > 1 ? wireChild(bytes, cardChildren, 1) : 0;
    const Span *separatorChildren = separator ? wireChildList(bytes, spans, count, separator) : NULL;
    int32_t separatorLine = separatorChildren && separatorChildren->childCount ? wireChild(bytes, separatorChildren, 0) : 0;
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
            wirePutChildren(out, newSeparator, (int32_t[]){newLine}, 1);
            children[childCount++] = newSeparator;
        }
        children[childCount++] = categoryRow(out, bytes, spans, count, source, index);
    }
    if (out->failed) return 0;
    wirePutChildren(out, newGroup, (int32_t[]){newHeader, newCard}, 2);
    wirePutChildren(out, newCard, children, childCount);
    gState.list = list;
    gState.active = 1;
    putListWithAstral(out, bytes, wireChildList(bytes, spans, count, list));
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
static int gHeaderHasPad;
static int gTitleHasInner;
static const char *gTemplateFailure = "no settings batch seen";

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
    const Span *list = wireChildList(bytes, spans, count, id);
    return list && list->childCount > position ? wireChild(bytes, list, position) : 0;
}

static const uint8_t *gTemplateSource;

void wireSetTemplateSource(const uint8_t *raw)
{
    gTemplateSource = raw;
}

static int storeTemplate(const uint8_t *bytes, const Span *spans, size_t count, int role, int32_t id)
{
    if (gTemplateSource) bytes = gTemplateSource;
    const Span *span = id ? wireCreation(spans, count, id) : NULL;
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
    ids[kCard] = wireParentOf(bytes, spans, count, row);
    ids[kGroup] = ids[kCard] ? wireParentOf(bytes, spans, count, ids[kCard]) : 0;
    ids[kList] = ids[kGroup] ? wireParentOf(bytes, spans, count, ids[kGroup]) : 0;
    ids[kScroll] = ids[kList] ? wireParentOf(bytes, spans, count, ids[kList]) : 0;
    ids[kContainer] = ids[kScroll] ? wireParentOf(bytes, spans, count, ids[kScroll]) : 0;
    ids[kColumn] = ids[kContainer] ? wireParentOf(bytes, spans, count, ids[kContainer]) : 0;
    ids[kPhone] = ids[kColumn] ? wireParentOf(bytes, spans, count, ids[kColumn]) : 0;
    ids[kRoot] = ids[kPhone] ? wireParentOf(bytes, spans, count, ids[kPhone]) : 0;
    ids[kSpacer] = firstChild(bytes, spans, count, ids[kContainer], 0);
    ids[kHairline] = firstChild(bytes, spans, count, ids[kContainer], 1);
    if (firstChild(bytes, spans, count, ids[kContainer], 2) != ids[kScroll]) {
        gTemplateFailure =
            ids[kScroll] ? "the scroll view is not the container's third child" : "the row's ancestors do not reach a scroll view";
        return;
    }
    ids[kHeaderBar] = firstChild(bytes, spans, count, ids[kContainer], 3);
    ids[kHeaderLeft] = firstChild(bytes, spans, count, ids[kHeaderBar], 0);
    ids[kHeaderLeftInner] = firstChild(bytes, spans, count, ids[kHeaderLeft], 0);
    ids[kHeaderLeftPad] = firstChild(bytes, spans, count, ids[kHeaderLeftInner], 0);
    ids[kBack] = firstChild(bytes, spans, count, ids[kHeaderLeftPad], 0);
    ids[kBackStack] = firstChild(bytes, spans, count, ids[kBack], 0);
    ids[kBackStackInner] = firstChild(bytes, spans, count, ids[kBackStack], 0);
    ids[kBackIcon] = firstChild(bytes, spans, count, ids[kBackStackInner], 0);
    int hasPad = ids[kBackIcon] != 0;
    if (!hasPad) {
        ids[kBack] = ids[kHeaderLeftPad];
        ids[kHeaderLeftPad] = 0;
        ids[kBackStack] = firstChild(bytes, spans, count, ids[kBack], 0);
        ids[kBackStackInner] = firstChild(bytes, spans, count, ids[kBackStack], 0);
        ids[kBackIcon] = firstChild(bytes, spans, count, ids[kBackStackInner], 0);
    }
    ids[kTitleWrap] = firstChild(bytes, spans, count, ids[kHeaderBar], 1);
    ids[kTitleInner] = firstChild(bytes, spans, count, ids[kTitleWrap], 0);
    ids[kTitle] = firstChild(bytes, spans, count, ids[kTitleInner], 0);
    int hasInner = ids[kTitle] != 0;
    if (!hasInner) {
        ids[kTitle] = ids[kTitleInner];
        ids[kTitleInner] = 0;
    }
    ids[kGroupHeader] = firstChild(bytes, spans, count, ids[kGroup], 0);
    ids[kSeparator] = firstChild(bytes, spans, count, ids[kCard], 1);
    ids[kSeparatorLine] = firstChild(bytes, spans, count, ids[kSeparator], 0);
    ids[kRowContent] = firstChild(bytes, spans, count, row, 0);
    ids[kRowIcon] = firstChild(bytes, spans, count, ids[kRowContent], 0);
    ids[kRowTitle] = firstChild(bytes, spans, count, ids[kRowContent], 1);
    ids[kRowTrailing] = firstChild(bytes, spans, count, ids[kRowContent], 2);
    ids[kRowChevron] = firstChild(bytes, spans, count, ids[kRowTrailing], 0);
    int32_t navigator = wireParentOf(bytes, spans, count, ids[kRoot]);
    if (!navigator || !navigatorWithId(navigator)) {
        gTemplateFailure = navigator ? "the settings root's parent is not a known navigator" : "the settings root has no parent";
        return;
    }
    for (int role = 0; role < kRoleCount; role++) {
        if ((role == kHeaderLeftPad && !hasPad) || (role == kTitleInner && !hasInner)) continue;
        if (!storeTemplate(bytes, spans, count, role, ids[role])) {
            static char missing[64];
            snprintf(missing, sizeof(missing), "settings template role %d was missing", role);
            gTemplateFailure = missing;
            return;
        }
    }
    gTemplateFailure = "";
    gSettingsNavigator = navigator;
    gHeaderHasPad = hasPad;
    gTitleHasInner = hasInner;
    gTemplatesReady = 1;
}

static int32_t integerProperty(const uint8_t *bytes, const Span *span, uint16_t wanted, int *found)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 3) {
            *found = 1;
            return (int32_t)wireRead32(&cursor);
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static void putPageRemoval(Out *out, const Page *page)
{
    wirePut8(out, 4);
    wirePut32(out, page->count);
    for (uint32_t index = 0; index < page->count; index++) wirePut32(out, (uint32_t)page->ids[index]);
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
        if (span->op == 1 && wireTypeIs(bytes, span, "navigator") && !navigatorWithId(span->id) && gNavigatorCount < kMaxNavigators) {
            gNavigators[gNavigatorCount++] = (Navigator){.id = span->id};
        }
        Navigator *navigator = (span->op == 2 || span->op == 3) ? navigatorWithId(span->id) : NULL;
        if (!navigator) continue;
        if (span->op == 3 && span->childCount <= kMaxNavigatorChildren) {
            navigator->count = span->childCount;
            for (uint32_t position = 0; position < span->childCount; position++)
                navigator->children[position] = wireChild(bytes, span, position);
            if (gDepth && navigator->id == gSettingsNavigator) {
                if (!*changed) wirePut(out, bytes, gBatchLength);
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
    wireCloneCreation(out, gTemplates[role].bytes, &span, id, changes, changeCount);
    return id;
}

static int32_t newNode(Out *out, const char *type)
{
    int32_t id = allocate();
    size_t length = strlen(type);
    wirePut8(out, 1);
    wirePut32(out, (uint32_t)id);
    wirePut16(out, (uint16_t)length);
    wirePut(out, type, length);
    return id;
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
    wirePropCount(out, P(accessibilityTapLabel));
    wirePropBool(out, P(accessibilityHidden), 1);
    wirePropString(out, P(accessibilityId), identifier);
    wirePropString(out, P(align), "stretch");
    if (on)
        wirePropLong(out, P(background), kSwitchOn);
    else
        wirePropInt(out, P(background), kSwitchOff);
    wirePropInt(out, P(borderColor), kSwitchBorder);
    wirePropNumber(out, P(borderWidth), 1);
    wirePropString(out, P(direction), "row");
    wirePropNumber(out, P(height), 28);
    wirePropEvents(out, P(on), "press");
    wirePropQuad(out, P(padding), 4, 4, 4, 4);
    wirePropString(out, P(pressFeedback), "none");
    wirePropNumber(out, P(radius), 16);
    wirePropNumber(out, P(width), 56);
    wirePropBool(out, P(exactHitArea), 1);
    int32_t thumb = newNode(out, "view");
    wirePropCount(out, P(accessibilityHint));
    wirePropString(out, P(align), "center");
    wirePropLong(out, P(background), kThumb);
    wirePropString(out, P(direction), "row");
    wirePropNumber(out, P(height), 20);
    wirePropString(out, P(justify), "center");
    wirePropNumber(out, P(radius), 9999);
    wirePropNumber(out, P(translateX), on ? 28 : 0);
    wirePropNumber(out, P(width), 20);
    wirePutChildren(out, track, (int32_t[]){thumb}, 1);
    return track;
}

static const int64_t kSliderActive = 0xFF5865F2;
static const int64_t kSliderInactive = 0xFF4E5058;

static int32_t buildSlider(Out *out, const AstralRowSpec *row)
{
    char identifier[128];
    snprintf(identifier, sizeof(identifier), "astral.slider.%d.%s", row->steps > 0 ? row->steps : 1, row->key);
    int32_t wrap = newNode(out, "view");
    wirePropCount(out, 3);
    wirePropString(out, P(align), "stretch");
    wirePropQuad(out, P(padding), 16, 0, 16, 12);
    wirePropString(out, P(width), "fill");
    int32_t slider = newNode(out, "slider");
    wirePropCount(out, 9);
    wirePropString(out, P(accessibilityId), identifier);
    wirePropString(out, P(accessibilityLabel), row->title);
    wirePropNumber(out, P(height), 40);
    wirePropString(out, P(width), "fill");
    wirePropInt(out, P(value), row->position);
    wirePropInt(out, P(steps), row->steps > 0 ? row->steps : 1);
    wirePropLong(out, P(minTrackTint), kSliderActive);
    wirePropLong(out, P(maxTrackTint), kSliderInactive);
    wirePropEvents(out, P(on), "change");
    wirePutChildren(out, wrap, (int32_t[]){slider}, 1);
    return wrap;
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
    if (row->kind == kAstralRowInfo || row->kind == kAstralRowNumber) rowChanges[used++] = (Change){P(on), kDrop, NULL, 0};
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
        wirePropCount(out, P(accessibilityChecked));
        wirePropString(out, P(direction), "column");
        wirePropNumber(out, P(flex), 1);
        wirePropNumber(out, P(gap), 2);
        Change subtitleChanges[] = {{P(text), kString, row->subtitle, 0},
                                    {P(fontSize), kNumber, NULL, 13},
                                    {P(lineHeight), kNumber, NULL, 17},
                                    {P(opacity), kNumber, NULL, 0.62},
                                    {P(maxLines), kInteger, NULL, 3}};
        int32_t subtitle = cloneRole(out, kRowTitle, subtitleChanges, 5);
        wirePutChildren(out, column, (int32_t[]){title, subtitle}, 2);
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
        wirePutChildren(out, trailing, (int32_t[]){chevron}, 1);
        children[childCount++] = trailing;
    } else if (row->kind == kAstralRowSelected) {
        int32_t trailing = cloneRole(out, kRowTrailing, NULL, 0);
        Change checkChange = {P(uri), kString, "res://ic_mana_checkmark_large_bold", 0};
        int32_t check = cloneRole(out, kRowChevron, &checkChange, 1);
        wirePutChildren(out, trailing, (int32_t[]){check}, 1);
        children[childCount++] = trailing;
    } else if (row->kind == kAstralRowSwitch || row->kind == kAstralRowSwitchLink) {
        int32_t trailing = newNode(out, "view");
        wirePropCount(out, P(accessibilityBusy));
        wirePropString(out, P(align), "stretch");
        wirePropQuad(out, P(margin), 18, 0, 0, 0);
        wirePutChildren(out, trailing, (int32_t[]){buildSwitch(out, row->key, row->on)}, 1);
        children[childCount++] = trailing;
    }
    wirePutChildren(out, content, children, childCount);
    if (row->kind == kAstralRowNumber) {
        int32_t column = newNode(out, "view");
        wirePropCount(out, 2);
        wirePropString(out, P(direction), "column");
        wirePropNumber(out, P(flex), 1);
        wirePutChildren(out, column, (int32_t[]){content, buildSlider(out, row)}, 2);
        wirePutChildren(out, rowId, (int32_t[]){column}, 1);
        return rowId;
    }
    wirePutChildren(out, rowId, (int32_t[]){content}, 1);
    return rowId;
}

static int64_t templateColor(int role, uint16_t wanted, int64_t fallback)
{
    if (!gTemplates[role].bytes) return fallback;
    Cursor cursor = {gTemplates[role].bytes, gTemplates[role].length, 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && (tag == 3 || tag == 4)) {
            uint32_t low = wireRead32(&cursor);
            return tag == 3 ? (int64_t)(int32_t)low : (int64_t)low;
        }
        wireSkipValue(&cursor, tag, 0);
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
    wirePropCount(out, (uint16_t)(6 + (lines > 0) + (grow > 0)));
    wirePropString(out, P(text), text);
    wirePropLong(out, P(color), color);
    wirePropNumber(out, P(fontSize), size);
    wirePropInt(out, P(fontWeight), weight);
    wirePropNumber(out, P(lineHeight), size + 4);
    wirePropNumber(out, P(opacity), opacity);
    if (lines > 0) wirePropInt(out, P(maxLines), lines);
    if (grow > 0) wirePropNumber(out, P(flex), 1);
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
    wirePropCount(out, P(accessibilityKeepsChildren));
    wirePropString(out, P(accessibilityId), identifier);
    wirePropString(out, P(align), "center");
    wirePropString(out, P(justify), "center");
    wirePropLong(out, P(background), kTranslucentFill);
    wirePropNumber(out, P(width), size);
    wirePropNumber(out, P(height), size);
    wirePropNumber(out, P(radius), size / 2);
    wirePropEvents(out, P(on), "press");
    wirePropString(out, P(role), "button");
    wirePropNumber(out, P(shrink), 0);
    wirePutChildren(out, button, (int32_t[]){buildIcon(out, icon, size * 0.55)}, 1);
    return button;
}

static int32_t buildChip(Out *out, const AstralChipSpec *chip)
{
    char identifier[128];
    snprintf(identifier, sizeof(identifier), "astral.row.%s", chip->key);
    int32_t node = newNode(out, "view");
    wirePropCount(out, P(accessibilityId));
    wirePropString(out, P(accessibilityId), identifier);
    wirePropString(out, P(align), "center");
    wirePropString(out, P(direction), "row");
    if (chip->selected)
        wirePropLong(out, P(background), kSwitchOn);
    else
        wirePropLong(out, P(background), kTranslucentFill);
    wirePropQuad(out, P(padding), 14, 7, 14, 7);
    wirePropNumber(out, P(radius), 16);
    wirePropEvents(out, P(on), "press");
    wirePropString(out, P(role), "button");
    wirePropNumber(out, P(shrink), 0);
    wirePutChildren(out, node, (int32_t[]){buildText(out, chip->title, 14, 600, 1, 1, 0)}, 1);
    return node;
}

static int32_t buildToolbar(Out *out, const AstralToolbarSpec *toolbar)
{
    int32_t children[12];
    uint32_t count = 0;
    for (size_t index = 0; index < toolbar->chipCount && count < 8; index++) children[count++] = buildChip(out, &toolbar->chips[index]);
    int32_t spacer = newNode(out, "view");
    wirePropCount(out, P(accessibilityActions));
    wirePropNumber(out, P(flex), 1);
    children[count++] = spacer;
    if (toolbar->buttonKey) children[count++] = buildRoundButton(out, toolbar->buttonKey, toolbar->buttonIcon, 36);
    int32_t row = newNode(out, "view");
    wirePropCount(out, P(accessibilityExpanded));
    wirePropString(out, P(direction), "row");
    wirePropString(out, P(align), "center");
    wirePropNumber(out, P(gap), 8);
    wirePropString(out, P(width), "fill");
    wirePropQuad(out, P(margin), 0, 0, 0, 4);
    wirePutChildren(out, row, children, count);
    return row;
}

static int32_t buildVersionTag(Out *out, const char *text)
{
    int32_t tag = newNode(out, "view");
    wirePropCount(out, P(accessibilityDisabled));
    wirePropLong(out, P(background), kTranslucentFill);
    wirePropQuad(out, P(padding), 6, 2, 6, 2);
    wirePropNumber(out, P(radius), 6);
    wirePropNumber(out, P(shrink), 0);
    wirePutChildren(out, tag, (int32_t[]){buildText(out, text, 11, 600, 0.62, 1, 0)}, 1);
    return tag;
}

static int32_t buildAvatar(Out *out, const char *source, int first)
{
    int32_t avatar = newNode(out, "image");
    wirePropCount(out, P(accessibilityHidden));
    wirePropString(out, P(uri), source);
    wirePropNumber(out, P(width), 24);
    wirePropNumber(out, P(height), 24);
    wirePropNumber(out, P(radius), 12);
    wirePropString(out, P(scale), "fill");
    wirePropQuad(out, P(margin), first ? 0 : -6, 0, 0, 0);
    wirePropNumber(out, P(shrink), 0);
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
        wirePropCount(out, P(accessibilityChecked));
        wirePropString(out, P(direction), "row");
        wirePropString(out, P(align), "center");
        wirePropNumber(out, P(shrink), 0);
        wirePutChildren(out, pile, avatars, (uint32_t)avatarCount);
        children[count++] = pile;
    }
    int32_t names = buildText(out, card->authors, 12, 500, 0.62, 1, 0);
    children[count++] = names;
    int32_t row = newNode(out, "view");
    wirePropCount(out, P(accessibilityChecked));
    wirePropString(out, P(direction), "row");
    wirePropString(out, P(align), "center");
    wirePropNumber(out, P(gap), 6);
    wirePutChildren(out, row, children, count);
    return row;
}

static int32_t buildCard(Out *out, const AstralCardSpec *card)
{
    char settingsKey[128], overflowKey[128];
    snprintf(settingsKey, sizeof(settingsKey), "settings.%s", card->key);
    snprintf(overflowKey, sizeof(overflowKey), "more.%s", card->key);
    int32_t tile = newNode(out, "view");
    wirePropCount(out, P(accessibilityHidden));
    wirePropString(out, P(align), "center");
    wirePropString(out, P(justify), "center");
    wirePropLong(out, P(background), kTranslucentFill);
    wirePropNumber(out, P(width), 40);
    wirePropNumber(out, P(height), 40);
    wirePropNumber(out, P(radius), 12);
    wirePropNumber(out, P(shrink), 0);
    wirePutChildren(out, tile, (int32_t[]){buildIcon(out, card->icon ? card->icon : "res://astral_plug", 22)}, 1);

    int32_t headingChildren[2];
    uint32_t headingCount = 0;
    headingChildren[headingCount++] = buildText(out, card->title, 16, 600, 1, 1, 0);
    if (card->authors && card->authors[0]) headingChildren[headingCount++] = buildAuthors(out, card);
    int32_t heading = newNode(out, "view");
    wirePropCount(out, P(accessibilityDisabled));
    wirePropString(out, P(direction), "column");
    wirePropNumber(out, P(flex), 1);
    wirePropNumber(out, P(gap), 2);
    wirePropNumber(out, P(minWidth), 0);
    wirePutChildren(out, heading, headingChildren, headingCount);

    int32_t actionChildren[3];
    uint32_t actionCount = 0;
    if (card->hasSettings) actionChildren[actionCount++] = buildRoundButton(out, settingsKey, "res://ic_mana_settings", 32);
    actionChildren[actionCount++] = buildRoundButton(out, overflowKey, "res://ic_mana_more_horizontal", 32);
    actionChildren[actionCount++] = buildSwitch(out, card->key, card->on);
    int32_t actions = newNode(out, "view");
    wirePropCount(out, P(accessibilityDisabled));
    wirePropString(out, P(direction), "row");
    wirePropString(out, P(align), "center");
    wirePropNumber(out, P(gap), 8);
    wirePropNumber(out, P(shrink), 0);
    wirePutChildren(out, actions, actionChildren, actionCount);

    int32_t top = newNode(out, "view");
    wirePropCount(out, P(accessibilityChecked));
    wirePropString(out, P(direction), "row");
    wirePropString(out, P(align), "start");
    wirePropNumber(out, P(gap), 12);
    wirePutChildren(out, top, (int32_t[]){tile, heading, actions}, 3);

    int32_t bottomChildren[2];
    uint32_t bottomCount = 0;
    if (card->description && card->description[0]) {
        bottomChildren[bottomCount++] = buildText(out, card->description, 14, 400, 1, 0, 1);
    } else {
        int32_t spacer = newNode(out, "view");
        wirePropCount(out, P(accessibilityActions));
        wirePropNumber(out, P(flex), 1);
        bottomChildren[bottomCount++] = spacer;
    }
    if (card->version && card->version[0]) bottomChildren[bottomCount++] = buildVersionTag(out, card->version);
    int32_t bottom = newNode(out, "view");
    wirePropCount(out, P(accessibilityChecked));
    wirePropString(out, P(direction), "row");
    wirePropString(out, P(align), "end");
    wirePropNumber(out, P(gap), 12);
    wirePutChildren(out, bottom, bottomChildren, bottomCount);

    int32_t cardChildren[3] = {top, bottom, 0};
    uint32_t cardCount = 2;
    if (card->failed && card->error && card->error[0]) {
        cardChildren[cardCount++] = buildColoredText(out, card->error, 12, 500, 1, 3, 0, kCriticalText);
    }
    int32_t node = newNode(out, "view");
    wirePropCount(out, P(accessibilityId));
    wirePropString(out, P(direction), "column");
    wirePropNumber(out, P(gap), 6);
    wirePropQuad(out, P(padding), 16, 16, 16, 16);
    wirePropLong(out, P(background), templateColor(kRow, P(background), 0x1497979F));
    wirePropNumber(out, P(radius), 16);
    wirePropLong(out, P(borderColor), kSubtleBorder);
    wirePropNumber(out, P(borderWidth), 1);
    wirePropNumber(out, P(opacity), card->failed ? 0.5 : 1);
    wirePropString(out, P(width), "fill");
    wirePutChildren(out, node, cardChildren, cardCount);
    return node;
}

static int32_t buildCardList(Out *out, const AstralPageSpec *page)
{
    int32_t children[66];
    uint32_t count = 0;
    if (page->toolbar) children[count++] = buildToolbar(out, page->toolbar);
    for (size_t index = 0; index < page->cardCount && count < 65; index++) children[count++] = buildCard(out, &page->cards[index]);
    int32_t list = newNode(out, "view");
    wirePropCount(out, P(accessibilityDisabled));
    wirePropString(out, P(direction), "column");
    wirePropNumber(out, P(gap), 12);
    wirePropString(out, P(width), "fill");
    wirePropQuad(out, P(margin), 0, 4, 0, 8);
    wirePutChildren(out, list, children, count);
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
                wirePutChildren(out, separator, (int32_t[]){line}, 1);
                cardChildren[cardCount++] = separator;
            }
            cardChildren[cardCount++] = buildSpecRow(out, &section->rows[row], row == 0, row == section->count - 1);
        }
        wirePutChildren(out, group, (int32_t[]){header, card}, 2);
        wirePutChildren(out, card, cardChildren, cardCount);
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
    wirePut8(&out, 1);
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
    int32_t leftPad = gHeaderHasPad ? cloneRole(&out, kHeaderLeftPad, NULL, 0) : 0;
    Change backChange = {P(accessibilityId), kString, "astral.back", 0};
    int32_t back = cloneRole(&out, kBack, &backChange, 1);
    int32_t backStack = cloneRole(&out, kBackStack, NULL, 0);
    int32_t backStackInner = cloneRole(&out, kBackStackInner, NULL, 0);
    Change backIconChange = {P(uri), kString, "res://ic_mana_arrow_large_left", 0};
    int32_t backIcon = cloneRole(&out, kBackIcon, &backIconChange, 1);
    int32_t titleWrap = cloneRole(&out, kTitleWrap, NULL, 0);
    int32_t titleInner = gTitleHasInner ? cloneRole(&out, kTitleInner, NULL, 0) : 0;
    Change titleChanges[] = {{P(accessibilityId), kString, "settings-astral-title", 0}, {P(text), kString, page->title, 0}};
    int32_t title = cloneRole(&out, kTitle, titleChanges, 2);

    wirePutChildren(&out, root, (int32_t[]){phone}, 1);
    wirePutChildren(&out, phone, (int32_t[]){column}, 1);
    wirePutChildren(&out, column, (int32_t[]){container}, 1);
    wirePutChildren(&out, container, (int32_t[]){spacer, hairline, scroll, headerBar}, 4);
    wirePutChildren(&out, scroll, (int32_t[]){list}, 1);
    wirePutChildren(&out, list, sections, sectionCount);
    wirePutChildren(&out, headerBar, (int32_t[]){left, titleWrap}, 2);
    wirePutChildren(&out, left, (int32_t[]){leftInner}, 1);
    if (leftPad) {
        wirePutChildren(&out, leftInner, (int32_t[]){leftPad}, 1);
        wirePutChildren(&out, leftPad, (int32_t[]){back}, 1);
    } else {
        wirePutChildren(&out, leftInner, (int32_t[]){back}, 1);
    }
    wirePutChildren(&out, back, (int32_t[]){backStack}, 1);
    wirePutChildren(&out, backStack, (int32_t[]){backStackInner}, 1);
    wirePutChildren(&out, backStackInner, (int32_t[]){backIcon}, 1);
    if (titleInner) {
        wirePutChildren(&out, titleWrap, (int32_t[]){titleInner}, 1);
        wirePutChildren(&out, titleInner, (int32_t[]){title}, 1);
    } else {
        wirePutChildren(&out, titleWrap, (int32_t[]){title}, 1);
    }

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
    wirePutChildren(&out, navigator->id, stack, navigator->count + 1);
    wirePut8(&out, 2);
    wirePut32(&out, (uint32_t)navigator->id);
    wirePut16(&out, 1);
    wirePut16(&out, P(index));
    wirePut8(&out, 3);
    wirePut32(&out, navigator->count);
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
    wirePut8(&out, 1);
    uint32_t contentStart = gPage->count;
    int32_t sections[16];
    uint32_t sectionCount = 0;
    buildSections(&out, page, sections, &sectionCount);
    wirePutChildren(&out, gPage->list, sections, sectionCount);
    wirePut8(&out, 4);
    wirePut32(&out, staleCount);
    for (uint32_t index = 0; index < staleCount; index++) wirePut32(&out, (uint32_t)stale[index]);
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
    wirePut8(&out, 1);
    wirePut8(&out, 2);
    wirePut32(&out, (uint32_t)navigator->id);
    wirePut16(&out, 1);
    wirePut16(&out, P(index));
    wirePut8(&out, 3);
    wirePut32(&out, (uint32_t)gPage->previousIndex);
    wirePutChildren(&out, navigator->id, gPage->previousChildren, gPage->previousCount);
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

void wireRewritePages(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    gBatchLength = length;
    trackNavigator(bytes, spans, count, out, changed);
    if (gState.active) {
        for (size_t index = 0; index < count; index++) {
            if (spans[index].op == 4 && removes(bytes, &spans[index], gState.list)) {
                if (!*changed) wirePut(out, bytes, length);
                putRemoval(out);
                if (gDepth) putAllPagesRemoval(out);
                gState.active = 0;
                *changed = 1;
                break;
            }
        }
    }
    if (gState.active) {
        const Span *list = wireChildList(bytes, spans, count, gState.list);
        if (list) {
            if (!*changed) wirePut(out, bytes, length);
            putListWithAstral(out, bytes, list);
            *changed = 1;
        }
        return;
    }
    for (size_t index = 0; index < count; index++) {
        if (spans[index].op != 1 || !wireStringProperty(bytes, &spans[index], P(accessibilityId), kAnchor)) continue;
        Out category = {0};
        captureTemplates(bytes, spans, count, spans[index].id);
        if (createCategory(&category, bytes, spans, count, &spans[index]) && !category.failed) {
            if (!*changed) wirePut(out, bytes, length);
            wirePut(out, category.bytes, category.length);
            *changed = 1;
        } else {
            gState.active = 0;
        }
        free(category.bytes);
        return;
    }
}

const char *astral_page_template_status(void)
{
    return gTemplatesReady ? "ready" : gTemplateFailure;
}

unsigned astral_page_navigator_count(void)
{
    return gNavigatorCount;
}
