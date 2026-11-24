#include "PhoenixWireInternal.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

enum { kMaxTweens = 64 };

static const double kTweenMs = 220;

typedef struct {
    int32_t node;
    double width;
    double inset;
    double started;
} Tween;

static Tween gTweens[kMaxTweens];

void wireTweenIn(int32_t node, double width, double inset)
{
    for (size_t index = 0; index < kMaxTweens; index++) {
        if (gTweens[index].node) continue;
        gTweens[index] = (Tween){node, width, inset, -1};
        return;
    }
}

void wireTweenForget(int32_t node)
{
    for (size_t index = 0; index < kMaxTweens; index++) {
        if (gTweens[index].node == node) gTweens[index].node = 0;
    }
}

int astral_wire_tweens_active(void)
{
    int active = 0;
    for (size_t index = 0; index < kMaxTweens; index++) active += gTweens[index].node != 0;
    return active;
}

static double eased(double progress)
{
    double remaining = 1 - progress;
    return 1 - remaining * remaining * remaining;
}

static void putFrame(Out *out, const Tween *tween, double amount)
{
    wirePut8(out, 2);
    wirePut32(out, (uint32_t)tween->node);
    wirePropCount(out, 3);
    wirePropNumber(out, P(opacity), amount);
    wirePropNumber(out, P(maxWidth), tween->width * amount);
    wirePropQuad(out, P(margin), 0, 0, (float)(tween->inset * amount), 0);
}

int astral_wire_tween_frame(AstralWireBuffer *buffer, double nowMs)
{
    memset(buffer, 0, sizeof(*buffer));
    if (!astral_wire_tweens_active()) return 0;
    Out out = {0};
    wirePut8(&out, kWireVersion);
    for (size_t index = 0; index < kMaxTweens; index++) {
        Tween *tween = &gTweens[index];
        if (!tween->node) continue;
        if (tween->started < 0) tween->started = nowMs;
        double progress = fmin(1, (nowMs - tween->started) / kTweenMs);
        putFrame(&out, tween, eased(progress));
        if (progress >= 1) tween->node = 0;
    }
    if (out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}
