#pragma once

#include <optional>

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

std::optional<Shape> fromSystem(std::optional<Frame> frame);
double cornerRadius(const Shape &shape);
Frame logoFrame(const Shape &shape);

}
