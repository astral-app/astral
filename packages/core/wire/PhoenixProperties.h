#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ASTRAL_WIRE_PROPERTIES(X)                                                                                                          \
    X(cacheKey, 113)                                                                                                                       \
    X(font, 205)                                                                                                                           \
    X(bold, 103)                                                                                                                           \
    X(strike, 487)                                                                                                                         \
    X(underline, 540)                                                                                                                      \
    X(accessibilityActions, 1)                                                                                                             \
    X(accessibilityBusy, 2)                                                                                                                \
    X(accessibilityChecked, 3)                                                                                                             \
    X(accessibilityDisabled, 4)                                                                                                            \
    X(accessibilityExpanded, 5)                                                                                                            \
    X(accessibilityHidden, 7)                                                                                                              \
    X(accessibilityHint, 8)                                                                                                                \
    X(accessibilityId, 9)                                                                                                                  \
    X(accessibilityKeepsChildren, 10)                                                                                                      \
    X(accessibilityLabel, 11)                                                                                                              \
    X(animateLayout, 72)                                                                                                                   \
    X(accessibilitySelected, 13)                                                                                                           \
    X(accessibilityTapLabel, 14)                                                                                                           \
    X(activeBackground, 21)                                                                                                                \
    X(activeBorderColor, 25)                                                                                                               \
    X(advance, 61)                                                                                                                         \
    X(align, 63)                                                                                                                           \
    X(animated, 73)                                                                                                                        \
    X(angle, 70)                                                                                                                           \
    X(background, 90)                                                                                                                      \
    X(backgroundGradient, 91)                                                                                                              \
    X(bg, 94)                                                                                                                              \
    X(bgFitText, 97)                                                                                                                       \
    X(bgRadius, 98)                                                                                                                        \
    X(borderColor, 104)                                                                                                                    \
    X(borderWidth, 107)                                                                                                                    \
    X(clip, 123)                                                                                                                           \
    X(changeText, 576)                                                                                                                     \
    X(color, 129)                                                                                                                          \
    X(count, 136)                                                                                                                          \
    X(direction, 147)                                                                                                                      \
    X(fade, 187)                                                                                                                           \
    X(fadeMs, 189)                                                                                                                         \
    X(first, 194)                                                                                                                          \
    X(flex, 198)                                                                                                                           \
    X(focusBorderColor, 201)                                                                                                               \
    X(fontSize, 207)                                                                                                                       \
    X(fontWeight, 208)                                                                                                                     \
    X(fullScreenBack, 218)                                                                                                                 \
    X(gap, 220)                                                                                                                            \
    X(height, 235)                                                                                                                         \
    X(image, 249)                                                                                                                          \
    X(includeFontPadding, 251)                                                                                                             \
    X(index, 252)                                                                                                                          \
    X(offset, 317)                                                                                                                         \
    X(justify, 260)                                                                                                                        \
    X(lineHeight, 278)                                                                                                                     \
    X(lineHeightExact, 279)                                                                                                                \
    X(margin, 287)                                                                                                                         \
    X(maxLines, 293)                                                                                                                       \
    X(maxTrackTint, 295)                                                                                                                   \
    X(maxWidth, 296)                                                                                                                       \
    X(minTrackTint, 300)                                                                                                                   \
    X(minWidth, 301)                                                                                                                       \
    X(presentation, 373)                                                                                                                   \
    X(on, 320)                                                                                                                             \
    X(opacity, 322)                                                                                                                        \
    X(padding, 332)                                                                                                                        \
    X(placeholder, 354)                                                                                                                    \
    X(placeholderColor, 355)                                                                                                               \
    X(pool, 366)                                                                                                                           \
    X(press, 614)                                                                                                                          \
    X(pressFeedback, 376)                                                                                                                  \
    X(radius, 388)                                                                                                                         \
    X(radiusBottomLeft, 389)                                                                                                               \
    X(radiusBottomRight, 390)                                                                                                              \
    X(radiusTopLeft, 391)                                                                                                                  \
    X(radiusTopRight, 392)                                                                                                                 \
    X(role, 414)                                                                                                                           \
    X(round, 418)                                                                                                                          \
    X(scale, 428)                                                                                                                          \
    X(shrink, 460)                                                                                                                         \
    X(size, 463)                                                                                                                           \
    X(spans, 468)                                                                                                                          \
    X(steps, 481)                                                                                                                          \
    X(text, 499)                                                                                                                           \
    X(textTransform, 508)                                                                                                                  \
    X(tint, 522)                                                                                                                           \
    X(translateX, 535)                                                                                                                     \
    X(uikitPush, 801)                                                                                                                      \
    X(uri, 542)                                                                                                                            \
    X(value, 543)                                                                                                                          \
    X(weight, 551)                                                                                                                         \
    X(width, 553)                                                                                                                          \
    X(baseline, 572)                                                                                                                       \
    X(colors, 577)                                                                                                                         \
    X(link, 598)                                                                                                                           \
    X(positions, 613)                                                                                                                      \
    X(exactHitArea, 805)                                                                                                                   \
    X(notoFallback, 835)

typedef struct {
#define ASTRAL_WIRE_FIELD(name, fallback) uint16_t name;
    ASTRAL_WIRE_PROPERTIES(ASTRAL_WIRE_FIELD)
#undef ASTRAL_WIRE_FIELD
} AstralWireProperties;

extern AstralWireProperties gAstralWireProperties;

#define P(name) (gAstralWireProperties.name)

size_t astral_wire_resolve_properties(const char *const *names, size_t count, const char **missing);
void astral_wire_reset_properties(void);

#ifdef __cplusplus
}
#endif
