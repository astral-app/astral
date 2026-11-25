#pragma once

#include <optional>

extern "C" {
typedef struct {
    double x;
    double y;
    double width;
    double height;
} AstralCutoutFrame;

int astral_cutout_kind(AstralCutoutFrame frame);
double astral_cutout_corner_radius(int kind, AstralCutoutFrame frame);
AstralCutoutFrame astral_cutout_logo_frame(int kind, AstralCutoutFrame frame);
}

namespace astral::cutout {
struct Frame {
    double x = 0;
    double y = 0;
    double width = 0;
    double height = 0;
};

enum class Kind {
    Island,
    Notch,
    Hole,
};

struct Shape {
    Kind kind;
    Frame frame;
};

inline AstralCutoutFrame bridged(const Frame &frame)
{
    return {frame.x, frame.y, frame.width, frame.height};
}

inline std::optional<Shape> fromSystem(std::optional<Frame> frame)
{
    if (!frame) return std::nullopt;
    int kind = astral_cutout_kind(bridged(*frame));
    if (kind < 0) return std::nullopt;
    return Shape{static_cast<Kind>(kind), *frame};
}

inline double cornerRadius(const Shape &shape)
{
    return astral_cutout_corner_radius(static_cast<int>(shape.kind), bridged(shape.frame));
}

inline Frame logoFrame(const Shape &shape)
{
    AstralCutoutFrame logo = astral_cutout_logo_frame(static_cast<int>(shape.kind), bridged(shape.frame));
    return {logo.x, logo.y, logo.width, logo.height};
}
} // namespace astral::cutout
