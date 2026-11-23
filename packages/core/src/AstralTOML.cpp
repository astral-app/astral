#define TOML_EXCEPTIONS 0
#include "AstralTOML.h"

#include <toml++/toml.hpp>

namespace {
nlohmann::json value(const toml::node &node)
{
    if (const auto *table = node.as_table()) {
        auto result = nlohmann::json::object();
        for (const auto &[key, entry] : *table) result[std::string(key.str())] = value(entry);
        return result;
    }
    if (const auto *array = node.as_array()) {
        auto result = nlohmann::json::array();
        for (const auto &entry : *array) result.push_back(value(entry));
        return result;
    }
    if (const auto *text = node.as_string()) return text->get();
    if (const auto *integer = node.as_integer()) return integer->get();
    if (const auto *number = node.as_floating_point()) return number->get();
    if (const auto *flag = node.as_boolean()) return flag->get();
    return nullptr;
}
}

nlohmann::json astral::parseTOML(std::string_view text)
{
    auto parsed = toml::parse(text);
    return parsed ? value(parsed.table()) : nlohmann::json();
}
