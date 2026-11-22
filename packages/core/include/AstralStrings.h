#pragma once

#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace astral {
using TextValues = std::vector<std::pair<std::string, std::string>>;

constexpr const char *kStringsFile = "i18n/astral.json";

void loadStrings(const std::string &documents);
void setStrings(const std::string &json);
std::string text(std::string_view key, std::string_view english, const TextValues &values = {});
} // namespace astral
