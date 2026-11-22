#import "TomlDocument.h"

#define TOML_EXCEPTIONS 0
#define TOML_HEADER_ONLY 1
#include <toml++/toml.hpp>

static id Value(const toml::node &node);

static NSDictionary *Table(const toml::table &table)
{
    NSMutableDictionary *dictionary = [NSMutableDictionary dictionaryWithCapacity:table.size()];
    for (const auto &[key, value] : table) {
        id converted = Value(value);
        NSString *name = [[NSString alloc] initWithBytes:key.str().data() length:key.str().size() encoding:NSUTF8StringEncoding];
        if (converted && name) dictionary[name] = converted;
    }
    return dictionary;
}

static id Value(const toml::node &node)
{
    if (const auto *table = node.as_table()) return Table(*table);
    if (const auto *array = node.as_array()) {
        NSMutableArray *items = [NSMutableArray arrayWithCapacity:array->size()];
        for (const auto &item : *array) {
            id converted = Value(item);
            if (converted) [items addObject:converted];
        }
        return items;
    }
    if (const auto *text = node.as_string()) {
        const std::string &value = text->get();
        return [[NSString alloc] initWithBytes:value.data() length:value.size() encoding:NSUTF8StringEncoding];
    }
    if (const auto *integer = node.as_integer()) return @(integer->get());
    if (const auto *number = node.as_floating_point()) return @(number->get());
    if (const auto *flag = node.as_boolean()) return @(flag->get());
    return nil;
}

NSDictionary *AstralParseTOML(NSData *data)
{
    if (!data.length) return nil;
    std::string_view text(static_cast<const char *>(data.bytes), data.length);
    toml::parse_result result = toml::parse(text);
    if (!result) return nil;
    return Table(result.table());
}
