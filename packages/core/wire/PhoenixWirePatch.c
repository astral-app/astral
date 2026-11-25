#include "PhoenixWireInternal.h"

#include <stdlib.h>
#include <string.h>

enum { kPatchPrefixCapacity = 32, kPatchPrefixLength = 96, kPatchedCapacity = 512, kRawCapacity = 40, kPatchProps = 11 };

typedef struct {
    uint8_t present;
    uint8_t tag;
    uint8_t length;
    uint8_t value[kRawCapacity];
} RawProp;

typedef struct {
    int32_t id;
    char identifier[kWireIdentifierLength];
    char type[kWireTypeLength];
    uint32_t applied;
    RawProp originals[kPatchProps];
} Patched;

static AstralNodePatcher gPatcher;
static char gPrefixes[kPatchPrefixCapacity][kPatchPrefixLength];
static size_t gPrefixCount;
static Patched gPatched[kPatchedCapacity];

static uint16_t patchProperty(size_t slot)
{
    const uint16_t properties[kPatchProps] = {P(background),  P(radius), P(opacity), P(padding), P(margin), P(borderColor),
                                              P(borderWidth), P(color),  P(width),   P(height),  P(clip)};
    return properties[slot];
}

static int gRefreshBusy;

void wireMarkRefreshBusy(void)
{
    gRefreshBusy = 1;
}

int astral_wire_take_refresh_busy(void)
{
    int busy = gRefreshBusy;
    gRefreshBusy = 0;
    return busy;
}

void astral_wire_set_node_patcher(AstralNodePatcher patcher)
{
    gPatcher = patcher;
}

void astral_wire_set_patch_prefixes(const char *const *prefixes, size_t count)
{
    gPrefixCount = 0;
    for (size_t index = 0; index < count && gPrefixCount < kPatchPrefixCapacity; index++) {
        if (!prefixes[index] || !prefixes[index][0] || strlen(prefixes[index]) >= kPatchPrefixLength) continue;
        strcpy(gPrefixes[gPrefixCount++], prefixes[index]);
    }
}

int wirePatchingActive(void)
{
    return gPatcher && gPrefixCount;
}

static size_t keyOffset(const char *identifier)
{
    for (size_t index = 0; index < gPrefixCount; index++) {
        size_t length = strlen(gPrefixes[index]);
        if (strncmp(identifier, gPrefixes[index], length) == 0) return length;
    }
    return 0;
}

uint64_t wireMessageOfIdentifier(const char *identifier)
{
    if (strncmp(identifier, "message.", 8) != 0) return 0;
    const char *colon = strchr(identifier, ':');
    if (!colon) return 0;
    uint64_t value = 0;
    size_t digits = 0;
    for (const char *cursor = colon + 1; *cursor >= '0' && *cursor <= '9'; cursor++, digits++)
        value = value * 10 + (uint64_t)(*cursor - '0');
    return digits >= 15 ? value : 0;
}

uint64_t astral_wire_message_of(const char *identifier)
{
    return wireMessageOfIdentifier(identifier);
}

static int callPatcher(int32_t node, const char *identifier, const char *type, AstralNodePatch *patch)
{
    uint64_t message = wireMessageOfIdentifier(identifier);
    if (!message && strcmp(identifier, "message-row") == 0) message = wireRowMessage(node);
    AstralPatchTarget target = {
        identifier, type, keyOffset(identifier), message, message ? wireMessageAuthor(message) : 0, astral_wire_current_channel()};
    return gPatcher(&target, patch);
}

int wireReadIdentifier(const uint8_t *bytes, const Span *span, char *identifier, char *type)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    uint16_t typeLength = wireRead16(&cursor);
    if (!wireNeed(&cursor, typeLength) || typeLength >= kWireTypeLength) return 0;
    memcpy(type, bytes + cursor.offset, typeLength);
    type[typeLength] = 0;
    cursor.offset += typeLength;
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == P(accessibilityId) && tag == 6) {
            uint32_t length = wireRead32(&cursor);
            if (!wireNeed(&cursor, length) || length >= kWireIdentifierLength) return 0;
            memcpy(identifier, bytes + cursor.offset, length);
            identifier[length] = 0;
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static int matchesPrefix(const char *identifier)
{
    for (size_t index = 0; index < gPrefixCount; index++) {
        if (strncmp(identifier, gPrefixes[index], strlen(gPrefixes[index])) == 0) return 1;
    }
    return 0;
}

static Patched *patchedSlot(int32_t id, int create)
{
    size_t start = ((uint32_t)id * 2654435761u) % kPatchedCapacity;
    for (size_t probe = 0; probe < kPatchedCapacity; probe++) {
        Patched *slot = &gPatched[(start + probe) % kPatchedCapacity];
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

static void captureOriginals(const uint8_t *bytes, const Span *span, Patched *patched)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        size_t start = cursor.offset;
        wireSkipValue(&cursor, tag, 0);
        size_t length = cursor.offset - start;
        for (size_t slot = 0; slot < kPatchProps; slot++) {
            if (patchProperty(slot) != property || length > kRawCapacity) continue;
            RawProp *raw = &patched->originals[slot];
            raw->present = 1;
            raw->tag = tag;
            raw->length = (uint8_t)length;
            memcpy(raw->value, bytes + start, length);
        }
    }
}

static uint32_t patchFields(const AstralNodePatch *patch)
{
    uint32_t fields = patch->fields;
    if (fields & AstralPatchHidden) fields |= AstralPatchOpacity | AstralPatchPadding | AstralPatchMargin;
    return fields;
}

static uint32_t slotMask(uint32_t fields)
{
    uint32_t mask = 0;
    if (fields & AstralPatchBackground) mask |= 1u << 0;
    if (fields & AstralPatchRadius) mask |= 1u << 1;
    if (fields & AstralPatchOpacity) mask |= 1u << 2;
    if (fields & AstralPatchPadding) mask |= 1u << 3;
    if (fields & AstralPatchMargin) mask |= 1u << 4;
    if (fields & AstralPatchBorder) mask |= (1u << 5) | (1u << 6);
    if (fields & AstralPatchTextColor) mask |= 1u << 7;
    if (fields & AstralPatchHidden) mask |= (1u << 8) | (1u << 9) | (1u << 10);
    return mask;
}

static void putPatchedValue(Out *out, size_t slot, const AstralNodePatch *patch)
{
    int hidden = (patch->fields & AstralPatchHidden) != 0;
    uint16_t property = patchProperty(slot);
    switch (slot) {
    case 0:
        wirePropLong(out, property, (int64_t)patch->background);
        break;
    case 1:
        wirePropNumber(out, property, patch->radius);
        break;
    case 2:
        wirePropNumber(out, property, hidden ? 0 : patch->opacity);
        break;
    case 3:
        if (hidden)
            wirePropQuad(out, property, 0, 0, 0, 0);
        else
            wirePropQuad(out, property, patch->padding[0], patch->padding[1], patch->padding[2], patch->padding[3]);
        break;
    case 4:
        if (hidden)
            wirePropQuad(out, property, 0, 0, 0, 0);
        else
            wirePropQuad(out, property, patch->margin[0], patch->margin[1], patch->margin[2], patch->margin[3]);
        break;
    case 5:
        wirePropLong(out, property, (int64_t)patch->borderColor);
        break;
    case 6:
        wirePropNumber(out, property, patch->borderWidth);
        break;
    case 7:
        wirePropLong(out, property, (int64_t)patch->textColor);
        break;
    case 8:
    case 9:
        wirePropNumber(out, property, 0);
        break;
    default:
        wirePropBool(out, property, 1);
        break;
    }
}

static void putOriginalValue(Out *out, size_t slot, const RawProp *raw)
{
    wirePut16(out, patchProperty(slot));
    if (!raw->present) {
        wirePut8(out, 0);
        return;
    }
    wirePut8(out, raw->tag);
    wirePut(out, raw->value, raw->length);
}

static int writePatch(Out *out, Patched *patched, const AstralNodePatch *patch)
{
    uint32_t wanted = slotMask(patchFields(patch));
    uint32_t touched = wanted | patched->applied;
    if (!touched) return 0;
    uint16_t count = 0;
    for (size_t slot = 0; slot < kPatchProps; slot++) count += (touched >> slot) & 1;
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)patched->id);
    wirePropCount(out, count);
    for (size_t slot = 0; slot < kPatchProps; slot++) {
        if (!((touched >> slot) & 1)) continue;
        if ((wanted >> slot) & 1)
            putPatchedValue(out, slot, patch);
        else
            putOriginalValue(out, slot, &patched->originals[slot]);
    }
    patched->applied = wanted;
    return 1;
}

static int evaluate(Out *out, Patched *patched)
{
    AstralNodePatch patch = {0};
    if (callPatcher(patched->id, patched->identifier, patched->type, &patch) < 0) {
        wireMarkRefreshBusy();
        return 0;
    }
    return writePatch(out, patched, &patch);
}

void wirePatchNodes(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gPatcher || !gPrefixCount) return;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 4) {
            for (uint32_t position = 0; position < span->childCount; position++) {
                Patched *removed = patchedSlot(wireChildAt(bytes, span, position), 0);
                if (removed) removed->id = -1;
            }
            continue;
        }
        if (span->op != 1) continue;
        char identifier[kWireIdentifierLength], type[kWireTypeLength];
        if (!wireReadIdentifier(bytes, span, identifier, type) || !matchesPrefix(identifier)) continue;
        Patched *patched = patchedSlot(span->id, 1);
        if (!patched) continue;
        strcpy(patched->identifier, identifier);
        strcpy(patched->type, type);
        captureOriginals(bytes, span, patched);
        AstralNodePatch patch = {0};
        int result = callPatcher(span->id, identifier, type, &patch);
        if (result < 0) wireMarkRefreshBusy();
        if (result <= 0) continue;
        if (!*changed) wirePut(out, bytes, length);
        *changed = 1;
        writePatch(out, patched, &patch);
    }
}

int astral_wire_refresh_node_patches(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    if (!gPatcher) return 0;
    Out out = {0};
    wirePut8(&out, kWireVersion);
    int any = 0;
    for (size_t index = 0; index < kPatchedCapacity; index++) {
        Patched *patched = &gPatched[index];
        if (patched->id > 0) any |= evaluate(&out, patched);
    }
    if (!any || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}
