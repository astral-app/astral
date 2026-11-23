#include "AstralCutout.h"

#include <algorithm>

namespace astral::cutout {

static constexpr double kNotchCornerRatio = 0.65;
static constexpr double kHoleLogoRatio = 0.6;
static constexpr double kLogoHeightRatio = 0.8;
static constexpr double kLogoAspect = 191.19 / 152.94;

std::optional<Shape> fromSystem(std::optional<Frame> frame)
{
    if (!frame || frame->width <= 0 || frame->height <= 0) return std::nullopt;
    if (frame->y <= 0) return Shape{Kind::Notch, *frame};
    return Shape{frame->width > frame->height ? Kind::Island : Kind::Hole, *frame};
}

double cornerRadius(const Shape &shape)
{
    if (shape.kind == Kind::Notch) return shape.frame.height * kNotchCornerRatio;
    if (shape.kind == Kind::Hole) return std::min(shape.frame.width, shape.frame.height) / 2;
    return shape.frame.height / 2;
}

Frame logoFrame(const Shape &shape)
{
    const Frame &frame = shape.frame;
    const double ratio = shape.kind == Kind::Hole ? kHoleLogoRatio : kLogoHeightRatio;
    const double height = std::min(frame.width, frame.height) * ratio;
    const double width = std::min(height * kLogoAspect, frame.width);
    const double fittedHeight = width / kLogoAspect;
    return {(frame.width - width) / 2, (frame.height - fittedHeight) / 2, width, fittedHeight};
}

}
