#include "PhoenixWireInternal.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { kMaxPalette = 1024 };

typedef struct {
    uint32_t from;
    uint32_t to;
} PaletteEntry;

static PaletteEntry gPalette[kMaxPalette];
static size_t gPaletteCount;

static int comparePaletteEntries(const void *left, const void *right)
{
    uint32_t a = ((const PaletteEntry *)left)->from, b = ((const PaletteEntry *)right)->from;
    return a < b ? -1 : a > b;
}

void astral_wire_set_palette(const uint32_t *from, const uint32_t *to, size_t count)
{
    size_t kept = 0;
    for (size_t index = 0; index < count && kept < kMaxPalette; index++) gPalette[kept++] = (PaletteEntry){from[index], to[index]};
    qsort(gPalette, kept, sizeof(*gPalette), comparePaletteEntries);
    size_t unique = 0;
    for (size_t index = 0; index < kept; index++) {
        if (unique && gPalette[unique - 1].from == gPalette[index].from) continue;
        gPalette[unique++] = gPalette[index];
    }
    gPaletteCount = unique;
}

static int paletteColor(uint32_t color, uint32_t *mapped)
{
    size_t low = 0, high = gPaletteCount;
    while (low < high) {
        size_t middle = (low + high) / 2;
        if (gPalette[middle].from == color) {
            *mapped = gPalette[middle].to;
            return 1;
        }
        if (gPalette[middle].from < color)
            low = middle + 1;
        else
            high = middle;
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

size_t wireBackgroundStops(uint32_t *colors, size_t capacity)
{
    size_t count = gBackgroundStopCount < capacity ? gBackgroundStopCount : capacity;
    for (size_t index = 0; index < count; index++) colors[index] = gBackgroundStops[index];
    return count;
}

static uint32_t backgroundStop(uint32_t index, uint32_t count)
{
    if (gBackgroundStopCount == 1 || count <= 1) return gBackgroundStops[0];
    size_t position = (size_t)index * (gBackgroundStopCount - 1) / (count - 1);
    return gBackgroundStops[position];
}

typedef struct {
    double hue;
    double saturation;
    double lightness;
} Hsl;

static Hsl toHsl(uint32_t argb)
{
    double red = ((argb >> 16) & 0xFF) / 255.0, green = ((argb >> 8) & 0xFF) / 255.0, blue = (argb & 0xFF) / 255.0;
    double high = fmax(red, fmax(green, blue)), low = fmin(red, fmin(green, blue)), chroma = high - low;
    Hsl hsl = {0, 0, (high + low) / 2};
    if (chroma <= 0) return hsl;
    hsl.saturation = chroma / (1 - fabs(2 * hsl.lightness - 1));
    if (high == red)
        hsl.hue = fmod((green - blue) / chroma, 6);
    else if (high == green)
        hsl.hue = (blue - red) / chroma + 2;
    else
        hsl.hue = (red - green) / chroma + 4;
    if (hsl.hue < 0) hsl.hue += 6;
    return hsl;
}

static uint32_t fromHsl(Hsl hsl, uint32_t alpha)
{
    double chroma = (1 - fabs(2 * hsl.lightness - 1)) * hsl.saturation;
    double second = chroma * (1 - fabs(fmod(hsl.hue, 2) - 1)), base = hsl.lightness - chroma / 2;
    double red = 0, green = 0, blue = 0;
    switch ((int)hsl.hue % 6) {
    case 0:
        red = chroma, green = second;
        break;
    case 1:
        red = second, green = chroma;
        break;
    case 2:
        green = chroma, blue = second;
        break;
    case 3:
        green = second, blue = chroma;
        break;
    case 4:
        red = second, blue = chroma;
        break;
    default:
        red = chroma, blue = second;
        break;
    }
    uint32_t r = (uint32_t)lround((red + base) * 255), g = (uint32_t)lround((green + base) * 255),
             b = (uint32_t)lround((blue + base) * 255);
    return alpha << 24 | (r > 255 ? 255 : r) << 16 | (g > 255 ? 255 : g) << 8 | (b > 255 ? 255 : b);
}

static uint32_t recolouredStop(uint32_t original, uint32_t theme)
{
    Hsl source = toHsl(original), target = toHsl(theme);
    target.lightness = source.lightness;
    uint32_t alpha = ((original >> 24) * (theme >> 24) + 127) / 255;
    return fromHsl(target, alpha);
}

typedef struct {
    uint32_t color;
    double saturation;
} SurfaceTint;

static uint32_t gTintColor;
static double gTintSaturation, gTintNeutral;
static SurfaceTint gSurfaceTints[kAstralSurfaceCount];

void astral_wire_set_tint(uint32_t color, double saturation, double neutral)
{
    gTintColor = color;
    gTintSaturation = saturation > 0 ? saturation : 0.45;
    gTintNeutral = neutral > 0 ? neutral : 0.3;
    memset(gSurfaceTints, 0, sizeof(gSurfaceTints));
}

void astral_wire_set_surface_tint(AstralSurface surface, uint32_t color, double saturation)
{
    if ((unsigned)surface >= kAstralSurfaceCount) return;
    gSurfaceTints[surface] = (SurfaceTint){color, saturation};
}

static double chromaOf(uint32_t argb)
{
    unsigned red = (argb >> 16) & 0xFF, green = (argb >> 8) & 0xFF, blue = argb & 0xFF;
    unsigned high = red > green ? (red > blue ? red : blue) : (green > blue ? green : blue);
    unsigned low = red < green ? (red < blue ? red : blue) : (green < blue ? green : blue);
    return (high - low) / 255.0;
}

static double neutralLimit(void)
{
    return gTintNeutral > 0 ? gTintNeutral : 0.3;
}

AstralSurface astral_wire_surface_of(uint32_t color)
{
    if (chromaOf(color) > neutralLimit()) return kAstralSurfaceAccents;
    return toHsl(color).lightness < 0.5 ? kAstralSurfaceBackgrounds : kAstralSurfaceText;
}

static int surfaceTint(uint32_t color, uint32_t *tint, double *saturation)
{
    AstralSurface surface = astral_wire_surface_of(color);
    const SurfaceTint *own = &gSurfaceTints[surface];
    if (own->color) {
        *tint = own->color;
        *saturation = own->saturation > 0 ? own->saturation : toHsl(own->color).saturation;
        return 1;
    }
    if (!gTintColor || surface == kAstralSurfaceAccents) return 0;
    *tint = gTintColor;
    *saturation = gTintSaturation;
    return 1;
}

static int tintedColor(uint32_t color, uint32_t *mapped)
{
    uint32_t tint;
    double saturation;
    if (!(color >> 24) || !surfaceTint(color, &tint, &saturation)) return 0;
    Hsl source = toHsl(color), target = toHsl(tint);
    target.lightness = source.lightness;
    target.saturation = saturation;
    *mapped = fromHsl(target, color >> 24);
    return 1;
}

static int anyTint(void)
{
    if (gTintColor) return 1;
    for (int surface = 0; surface < kAstralSurfaceCount; surface++) {
        if (gSurfaceTints[surface].color) return 1;
    }
    return 0;
}

static int themedColor(uint32_t color, uint32_t *mapped)
{
    return paletteColor(color, mapped) || tintedColor(color, mapped);
}

static int patchGradientStops(uint8_t *bytes, Cursor *cursor, int replaceAll)
{
    int patched = 0;
    uint32_t entries = wireRead32(cursor);
    for (uint32_t entry = 0; entry < entries && !cursor->failed; entry++) {
        uint16_t key = wireRead16(cursor);
        uint8_t tag = wireRead8(cursor);
        if (key != P(colors) || tag != 7) {
            wireSkipValue(cursor, tag, 1);
            continue;
        }
        uint32_t count = wireRead32(cursor);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            uint8_t stopTag = wireRead8(cursor);
            size_t at = cursor->offset;
            wireSkipValue(cursor, stopTag, 2);
            if (cursor->failed || (stopTag != 3 && stopTag != 4)) continue;
            uint32_t color, mapped;
            memcpy(&color, bytes + at, 4);
            if (replaceAll)
                mapped = recolouredStop(color, backgroundStop(index, count));
            else if (!themedColor(color, &mapped))
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

int wirePatchColors(uint8_t *bytes, const Span *spans, size_t count)
{
    int patched = 0;
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2) continue;
        Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
        if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
        uint16_t properties = wireRead16(&cursor);
        for (uint16_t entry = 0; entry < properties && !cursor.failed; entry++) {
            uint16_t property = wireRead16(&cursor);
            uint8_t tag = wireRead8(&cursor);
            size_t at = cursor.offset;
            wireSkipValue(&cursor, tag, 0);
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
                if ((replaceAll || gPaletteCount || anyTint()) && patchGradientStops(bytes, &gradient, replaceAll)) patched = 1;
                continue;
            }
            if (cursor.failed || !colorProperty(property) || (tag != 3 && tag != 4)) continue;
            uint32_t color, mapped;
            memcpy(&color, bytes + at, 4);
            if (!themedColor(color, &mapped)) continue;
            memcpy(bytes + at, &mapped, 4);
            if (tag == 4) memset(bytes + at + 4, 0, 4);
            patched = 1;
        }
    }
    return patched;
}

enum { kColorCapacity = 32768, kColorEmpty = 0, kColorLive = 1, kColorGone = 2 };
enum { kTrackedColor, kTrackedNumber, kTrackedGradient };

typedef struct {
    int32_t id;
    uint16_t property;
    uint8_t state;
    uint8_t tag;
    uint8_t kind;
    uint32_t length;
    union {
        uint32_t color;
        double number;
        uint8_t *bytes;
    } value;
} TrackedValue;

static TrackedValue gColors[kColorCapacity];
static size_t gColorsOccupied;
static size_t gColorsGone;
static unsigned gColorsDropped;

static int scalableProperty(uint16_t property)
{
    return property == P(radius) || property == P(radiusBottomLeft) || property == P(radiusBottomRight) || property == P(radiusTopLeft) ||
           property == P(radiusTopRight) || property == P(fontSize) || property == P(lineHeight);
}

static double currentScale(uint16_t property)
{
    return property == P(fontSize) || property == P(lineHeight) ? gFontScale : gRadiusScale;
}

static size_t colorSlot(int32_t id, uint16_t property)
{
    return ((uint32_t)id * 2654435761u ^ property * 40503u) & (kColorCapacity - 1);
}

static void releaseValue(TrackedValue *tracked)
{
    if (tracked->kind == kTrackedGradient) free(tracked->value.bytes);
    tracked->value.bytes = NULL;
    tracked->state = kColorGone;
    gColorsGone++;
}

static void compactColors(void)
{
    static TrackedValue live[kColorCapacity];
    size_t count = 0;
    for (size_t index = 0; index < kColorCapacity; index++) {
        if (gColors[index].state == kColorLive) live[count++] = gColors[index];
    }
    memset(gColors, 0, sizeof(gColors));
    gColorsOccupied = 0;
    gColorsGone = 0;
    for (size_t index = 0; index < count; index++) {
        size_t slot = colorSlot(live[index].id, live[index].property);
        while (gColors[slot].state != kColorEmpty) slot = (slot + 1) & (kColorCapacity - 1);
        gColors[slot] = live[index];
        gColorsOccupied++;
    }
}

static TrackedValue *trackedSlot(int32_t id, uint16_t property)
{
    if (gColorsOccupied >= kColorCapacity / 2 && gColorsGone >= gColorsOccupied / 4) compactColors();
    if (gColorsOccupied >= kColorCapacity * 3 / 4) {
        gColorsDropped++;
        return NULL;
    }
    size_t slot = colorSlot(id, property);
    size_t reusable = kColorCapacity;
    while (gColors[slot].state != kColorEmpty) {
        if (gColors[slot].state == kColorLive && gColors[slot].id == id && gColors[slot].property == property) {
            if (gColors[slot].kind == kTrackedGradient) free(gColors[slot].value.bytes);
            return &gColors[slot];
        }
        if (gColors[slot].state == kColorGone && reusable == kColorCapacity) reusable = slot;
        slot = (slot + 1) & (kColorCapacity - 1);
    }
    if (reusable == kColorCapacity) {
        reusable = slot;
        gColorsOccupied++;
    } else {
        gColorsGone--;
    }
    return &gColors[reusable];
}

static void rememberValue(int32_t id, uint16_t property, uint8_t tag, const uint8_t *value, size_t length)
{
    TrackedValue *tracked = trackedSlot(id, property);
    if (!tracked) return;
    *tracked = (TrackedValue){.id = id, .property = property, .state = kColorLive, .tag = tag};
    if (tag == 3 || tag == 4) {
        tracked->kind = kTrackedColor;
        memcpy(&tracked->value.color, value, 4);
    } else if (tag == 5) {
        tracked->kind = kTrackedNumber;
        memcpy(&tracked->value.number, value, 8);
    } else {
        tracked->kind = kTrackedGradient;
        tracked->value.bytes = malloc(length);
        if (!tracked->value.bytes) {
            tracked->state = kColorGone;
            return;
        }
        memcpy(tracked->value.bytes, value, length);
        tracked->length = (uint32_t)length;
    }
}

static void forgetColor(int32_t id, uint16_t property)
{
    size_t slot = colorSlot(id, property);
    while (gColors[slot].state != kColorEmpty) {
        if (gColors[slot].state == kColorLive && gColors[slot].id == id && gColors[slot].property == property) {
            releaseValue(&gColors[slot]);
            return;
        }
        slot = (slot + 1) & (kColorCapacity - 1);
    }
}

static void forgetRemovedColors(const uint8_t *bytes, const Span *span)
{
    const uint16_t properties[] = {
        P(background),       P(color),    P(borderColor), P(activeBackground),  P(activeBorderColor), P(focusBorderColor),
        P(placeholderColor), P(tint),     P(radius),      P(radiusBottomLeft),  P(radiusBottomRight), P(radiusTopLeft),
        P(radiusTopRight),   P(fontSize), P(lineHeight),  P(backgroundGradient)};
    for (uint32_t position = 0; position < span->childCount; position++) {
        int32_t id = wireChildAt(bytes, span, position);
        for (size_t index = 0; index < sizeof(properties) / sizeof(*properties); index++) forgetColor(id, properties[index]);
    }
}

static int trackedProperty(uint16_t property, uint8_t tag)
{
    if (tag == 3 || tag == 4) return colorProperty(property);
    if (tag == 5) return scalableProperty(property);
    return tag == 8 && property == P(backgroundGradient);
}

void wireTrackColors(const uint8_t *bytes, const Span *spans, size_t count)
{
    for (size_t index = 0; index < count; index++) {
        const Span *span = &spans[index];
        if (span->op == 4 && span->childCount) forgetRemovedColors(bytes, span);
        if ((span->op != 1 && span->op != 2) || span->id <= 0 || span->id >= kFirstId) continue;
        Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
        if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
        uint16_t properties = wireRead16(&cursor);
        for (uint16_t entry = 0; entry < properties && !cursor.failed; entry++) {
            uint16_t property = wireRead16(&cursor);
            uint8_t tag = wireRead8(&cursor);
            size_t at = cursor.offset;
            wireSkipValue(&cursor, tag, 0);
            if (!cursor.failed && trackedProperty(property, tag)) rememberValue(span->id, property, tag, bytes + at, cursor.offset - at);
        }
    }
}

static void putTrackedValue(Out *out, const TrackedValue *tracked)
{
    if (tracked->kind == kTrackedColor) {
        uint32_t color = tracked->value.color;
        themedColor(tracked->value.color, &color);
        wirePut32(out, color);
        if (tracked->tag == 4) wirePut32(out, 0);
        return;
    }
    if (tracked->kind == kTrackedNumber) {
        double value = tracked->value.number;
        if (value > 0 && value < 9000) value *= currentScale(tracked->property);
        wirePut(out, &value, 8);
        return;
    }
    uint8_t *copy = malloc(tracked->length);
    if (!copy) {
        out->failed = 1;
        return;
    }
    memcpy(copy, tracked->value.bytes, tracked->length);
    Cursor gradient = {copy, tracked->length, 0, 0};
    if (gBackgroundStopCount > 0 || gPaletteCount || anyTint()) patchGradientStops(copy, &gradient, gBackgroundStopCount > 0);
    wirePut(out, copy, tracked->length);
    free(copy);
}

int astral_wire_recolor(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    Out out = {0};
    wirePut8(&out, kWireVersion);
    size_t updates = 0;
    for (size_t index = 0; index < kColorCapacity; index++) {
        const TrackedValue *tracked = &gColors[index];
        if (tracked->state != kColorLive) continue;
        wirePut8(&out, 2);
        wirePut32(&out, (uint32_t)tracked->id);
        wirePut16(&out, 1);
        wirePut16(&out, tracked->property);
        wirePut8(&out, tracked->tag);
        putTrackedValue(&out, tracked);
        updates++;
    }
    if (!updates || out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return updates > INT32_MAX ? INT32_MAX : (int)updates;
}

unsigned astral_wire_colors_dropped(void)
{
    return gColorsDropped;
}

void astral_wire_forget_colors(void)
{
    for (size_t index = 0; index < kColorCapacity; index++) {
        if (gColors[index].state == kColorLive) releaseValue(&gColors[index]);
    }
    memset(gColors, 0, sizeof(gColors));
    gColorsOccupied = 0;
    gColorsGone = 0;
    gColorsDropped = 0;
}

int wireThemeActive(void)
{
    return gPaletteCount || anyTint() || gRadiusScale != 1 || gFontScale != 1 || gBackgroundStopCount;
}
