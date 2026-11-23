#pragma once

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace astral::navigator {

using Deliver = std::function<void(std::vector<uint8_t> bytes)>;

void configure(Deliver deliver, std::function<void(const std::function<void()> &)> withWire);
bool handles(const std::string &identifier);
void tap(const std::string &identifier);
void slide(const std::string &identifier, double fraction);
void slidePosition(const std::string &identifier, double position);
bool back();
void refresh();
bool open();

} // namespace astral::navigator
