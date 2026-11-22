#include "PhoenixWireInternal.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 5) {
            if (!wireNeed(&cursor, 8)) return 0;
            memcpy(value, bytes + cursor.offset, 8);
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static int stringAt(const uint8_t *bytes, const Span *span, uint16_t wanted, size_t *offset, uint32_t *length)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted && tag == 6) {
            size_t at = cursor.offset;
            uint32_t size = wireRead32(&cursor);
            if (!wireNeed(&cursor, size)) return 0;
            *offset = at;
            *length = size;
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static int firstSpanText(const uint8_t *bytes, const Span *span, size_t *offset, uint32_t *length)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property != P(spans) || tag != 7) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        if (wireRead32(&cursor) == 0 || wireRead8(&cursor) != 8) return 0;
        uint32_t entries = wireRead32(&cursor);
        for (uint32_t entry = 0; entry < entries && !cursor.failed; entry++) {
            uint16_t key = wireRead16(&cursor);
            uint8_t valueTag = wireRead8(&cursor);
            if (key == 499 && valueTag == 6) {
                size_t at = cursor.offset;
                uint32_t size = wireRead32(&cursor);
                if (!wireNeed(&cursor, size)) return 0;
                *offset = at;
                *length = size;
                return 1;
            }
            wireSkipValue(&cursor, valueTag, 0);
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
        if (wireStringProperty(bytes, span, P(accessibilityId), "profile-scroll")) profile = 1;
        if (!user && wireTypeIs(bytes, span, "image") && stringAt(bytes, span, P(uri), &offset, &size))
            user = wireAvatarUser(bytes + offset + 4, size);
    }
    if (!profile) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        double size = 0;
        size_t offset;
        uint32_t length;
        if (span->op != 1 || !wireTypeIs(bytes, span, "text") || !numberAt(bytes, span, P(fontSize), &size) || size < kProfileNameSize)
            continue;
        if (!stringAt(bytes, span, P(text), &offset, &length)) continue;
        AstralLabelContext context = {kAstralLabelProfile, 0, astral_wire_current_channel(), user};
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
            AstralLabelContext context = {kAstralLabelComposer, 0, astral_wire_current_channel(), 0};
            addLabelEdit(edits, editCount, bytes, offset, size, &context);
            continue;
        }
        for (size_t kind = 0; kind < sizeof(kLabelRows) / sizeof(*kLabelRows); kind++) {
            uint64_t target = 0;
            if (!wirePrefixed(bytes + offset + 4, size, kLabelRows[kind].prefix, &target)) continue;
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
        if (span->op != 1 || !wireTypeIs(bytes, span, "text")) continue;
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
                uint64_t channel = rowKinds[row] == kAstralLabelChannel ? rowTargets[row] : astral_wire_current_channel();
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

uint8_t *wireRelabel(const uint8_t *bytes, size_t length, Span *spans, size_t count, size_t *newLength)
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
                links[linkCount++] = (LabelLink){wireChildAt(bytes, span, position), span->id};
            }
        } else if (span->op == 1) {
            if (wireStringProperty(bytes, span, P(accessibilityId), "message-row") && rowCount < kMaxLabelNodes)
                rows[rowCount++] = span->id;
            size_t offset;
            uint32_t size;
            uint64_t message = 0;
            if (stringAt(bytes, span, P(accessibilityId), &offset, &size) &&
                wirePrefixed(bytes + offset + 4, size, "message.author:", &message) && authorCount < kMaxLabelNodes) {
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
        Tracked *tracked = message ? NULL : wireTrackSlot(span->id, 0);
        if (!message && tracked && tracked->kind == kTrackAuthor) message = tracked->value;
        if (message && stringAt(bytes, span, P(text), &offset, &size)) {
            AstralLabelContext context = {kAstralLabelAuthor, message, astral_wire_current_channel(), 0};
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
        AstralLabelContext context = {kAstralLabelReply, replying, astral_wire_current_channel(), 0};
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
