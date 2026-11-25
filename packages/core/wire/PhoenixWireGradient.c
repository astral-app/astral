#include "PhoenixWireInternal.h"

void wirePropGradient(Out *out, uint16_t property, double angle, const uint32_t *colors, uint32_t count)
{
    wirePut16(out, property);
    wirePut8(out, 8);
    wirePut32(out, 3);
    wirePut16(out, P(angle));
    wirePut8(out, 5);
    wirePut(out, &angle, 8);
    wirePut16(out, P(colors));
    wirePut8(out, 7);
    wirePut32(out, count);
    for (uint32_t index = 0; index < count; index++) {
        wirePut8(out, 4);
        int64_t color = colors[index];
        wirePut(out, &color, 8);
    }
    wirePut16(out, P(positions));
    wirePut8(out, 7);
    wirePut32(out, count);
    for (uint32_t index = 0; index < count; index++) {
        wirePut8(out, 5);
        double position = count > 1 ? (double)index / (count - 1) : 0;
        wirePut(out, &position, 8);
    }
}
