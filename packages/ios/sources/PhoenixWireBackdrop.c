#include "PhoenixWireInternal.h"

#include <math.h>

enum { kBackdropDepth = 4, kBackdropStops = 8, kBackdropTargets = 64 };

static const char kSettingsScreen[] = "settings-phone";

static int gBackdropEnabled;
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

static size_t dimmedStops(uint32_t *colors)
{
    size_t count = wireBackgroundStops(colors, kBackdropStops);
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
    wirePropGradient(out, P(backgroundGradient), 180, stops, (uint32_t)stopCount);
}

void wireSettingsBackdrop(const uint8_t *bytes, const Span *spans, size_t count, Out *out, int *changed, size_t length)
{
    if (!gBackdropEnabled || !wireThemeActive()) return;
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
