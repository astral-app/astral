#include "PhoenixWireInternal.h"

#include <math.h>

enum { kBackdropDepth = 4, kBackdropStops = 8, kBackdropTargets = 64 };

static const char kSettingsScreen[] = "settings-phone";

static int gBackdropEnabled;
static int32_t gAppGradientNode;
static uint32_t gAppStops[kBackdropStops];
static size_t gAppStopCount;
static double gAppAngle = 180;
static double gBackdropDim = 0.55;

void astral_wire_set_settings_backdrop(int enabled, double dim)
{
    gBackdropEnabled = enabled;
    gBackdropDim = dim < 0 ? 0 : dim > 0.9 ? 0.9 : dim;
}

int astral_wire_settings_backdrop_enabled(void)
{
    return gBackdropEnabled;
}

static uint32_t dimmed(uint32_t color, double dim)
{
    double keep = 1 - dim;
    uint32_t red = (uint32_t)lround(((color >> 16) & 0xFF) * keep);
    uint32_t green = (uint32_t)lround(((color >> 8) & 0xFF) * keep);
    uint32_t blue = (uint32_t)lround((color & 0xFF) * keep);
    return 0xFF000000u | red << 16 | green << 8 | blue;
}

static int propertiesCursor(const uint8_t *bytes, const Span *span, Cursor *cursor)
{
    *cursor = (Cursor){bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(cursor, wireRead16(cursor));
    return !cursor->failed;
}

static int readGradient(const uint8_t *bytes, const Span *span, double *angle, uint32_t *stops, size_t *stopCount)
{
    Cursor cursor;
    if (!propertiesCursor(bytes, span, &cursor)) return 0;
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property != P(backgroundGradient) || tag != 8) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        uint32_t entries = wireRead32(&cursor);
        size_t found = 0;
        for (uint32_t entry = 0; entry < entries && !cursor.failed; entry++) {
            uint16_t key = wireRead16(&cursor);
            uint8_t valueTag = wireRead8(&cursor);
            if (key == P(angle) && valueTag == 5 && wireNeed(&cursor, 8)) {
                memcpy(angle, cursor.bytes + cursor.offset, 8);
                cursor.offset += 8;
            } else if (key == P(colors) && valueTag == 7) {
                uint32_t colors = wireRead32(&cursor);
                for (uint32_t stop = 0; stop < colors && !cursor.failed; stop++) {
                    uint8_t stopTag = wireRead8(&cursor);
                    size_t at = cursor.offset;
                    wireSkipValue(&cursor, stopTag, 2);
                    if (cursor.failed || (stopTag != 3 && stopTag != 4) || found >= kBackdropStops) continue;
                    memcpy(&stops[found++], bytes + at, 4);
                }
            } else {
                wireSkipValue(&cursor, valueTag, 1);
            }
        }
        *stopCount = found;
        return !cursor.failed && found > 0;
    }
    return 0;
}

static int hasProperty(const uint8_t *bytes, const Span *span, uint16_t wanted)
{
    Cursor cursor;
    if (!propertiesCursor(bytes, span, &cursor)) return 0;
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == wanted) return 1;
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static void observeAppGradient(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 1 && gAppGradientNode && span->id != gAppGradientNode) continue;
        if (span->op == 2 && span->id != gAppGradientNode) continue;
        if (span->op != 1 && span->op != 2) continue;
        if (span->op == 1 && !wireTypeIs(bytes, span, "stack")) continue;
        double angle = 180;
        uint32_t stops[kBackdropStops];
        size_t stopCount = 0;
        if (!readGradient(bytes, span, &angle, stops, &stopCount)) {
            if (span->op == 2 && hasProperty(bytes, span, P(backgroundGradient))) gAppStopCount = 0;
            continue;
        }
        gAppGradientNode = span->id;
        gAppAngle = angle;
        gAppStopCount = stopCount;
        memcpy(gAppStops, stops, stopCount * sizeof(uint32_t));
    }
}

size_t astral_wire_app_gradient(uint32_t *colors, size_t capacity)
{
    size_t count = gAppStopCount < capacity ? gAppStopCount : capacity;
    memcpy(colors, gAppStops, count * sizeof(uint32_t));
    return count;
}

static size_t dimmedStops(uint32_t *colors)
{
    size_t count = gAppStopCount;
    if (count)
        memcpy(colors, gAppStops, count * sizeof(uint32_t));
    else
        count = wireBackgroundStops(colors, kBackdropStops);
    for (size_t index = 0; index < count; index++) colors[index] = dimmed(colors[index], gBackdropDim);
    return count;
}

static void collectMatching(const uint8_t *bytes, const Span *spans, size_t count, int32_t id, uint32_t background, int depth,
                            int32_t *targets, size_t *found)
{
    if (depth > kBackdropDepth || *found >= kBackdropTargets) return;
    const Span *list = wireChildList(bytes, spans, count, id);
    if (!list) return;
    for (uint32_t position = 0; position < list->childCount && *found < kBackdropTargets; position++) {
        int32_t child = wireChildAt(bytes, list, position);
        const Span *creation = wireCreation(spans, count, child);
        uint32_t color;
        if (creation && wireColorProperty(bytes, creation, P(background), &color) && color == background) targets[(*found)++] = child;
        collectMatching(bytes, spans, count, child, background, depth + 1, targets, found);
    }
}

static void putTransparent(Out *out, int32_t id)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)id);
    wirePut16(out, 1);
    wirePropLong(out, P(background), 0);
}

static void putBackdrop(Out *out, int32_t id, const uint32_t *stops, size_t stopCount)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)id);
    wirePut16(out, 2);
    wirePropLong(out, P(background), 0);
    wirePropGradient(out, P(backgroundGradient), gAppStopCount ? gAppAngle : 180, stops, (uint32_t)stopCount);
}

void wireSettingsBackdrop(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    observeAppGradient(bytes, spans, count);
    if (!gBackdropEnabled || (!gAppStopCount && !wireThemeActive())) return;
    uint32_t stops[kBackdropStops];
    size_t stopCount = dimmedStops(stops);
    if (stopCount == 0) return;
    if (stopCount == 1) stops[stopCount++] = stops[0];
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        uint32_t background;
        if (span->op != 1 || !wireTypeIs(bytes, span, "view") || !wireStringProperty(bytes, span, P(accessibilityId), kSettingsScreen) ||
            !wireColorProperty(bytes, span, P(background), &background))
            continue;
        int32_t targets[kBackdropTargets];
        size_t found = 0;
        collectMatching(bytes, spans, count, span->id, background, 1, targets, &found);
        if (!*changed) wirePut(out, bytes, length);
        *changed = 1;
        putBackdrop(out, span->id, stops, stopCount);
        for (size_t target = 0; target < found; target++) putTransparent(out, targets[target]);
    }
}
