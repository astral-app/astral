#import "FoundationJSON.h"

static const int kMaxDepth = 6;

id AstralObjectFromJSON(const nlohmann::json &value)
{
    if (value.is_string()) return [NSString stringWithUTF8String:value.get_ref<const std::string &>().c_str()] ?: @"";
    if (value.is_boolean()) return @(value.get<bool>());
    if (value.is_number_integer()) return @(value.get<int64_t>());
    if (value.is_number_unsigned()) return @(value.get<uint64_t>());
    if (value.is_number_float()) return @(value.get<double>());
    if (value.is_array()) {
        NSMutableArray *array = [NSMutableArray arrayWithCapacity:value.size()];
        for (const auto &item : value) [array addObject:AstralObjectFromJSON(item) ?: NSNull.null];
        return array;
    }
    if (value.is_object()) {
        NSMutableDictionary *dictionary = [NSMutableDictionary dictionaryWithCapacity:value.size()];
        for (auto entry = value.begin(); entry != value.end(); ++entry) {
            id converted = AstralObjectFromJSON(entry.value());
            NSString *key = [NSString stringWithUTF8String:entry.key().c_str()];
            if (converted && key) dictionary[key] = converted;
        }
        return dictionary;
    }
    return nil;
}

static BOOL IsBoolean(NSNumber *number)
{
    return strcmp(number.objCType, @encode(BOOL)) == 0 || strcmp(number.objCType, "c") == 0;
}

nlohmann::json AstralJSONFromObject(id value, int depth)
{
    if (depth > kMaxDepth || !value || value == NSNull.null) return nullptr;
    if ([value isKindOfClass:NSString.class]) return std::string([value UTF8String] ?: "");
    if ([value isKindOfClass:NSNumber.class]) {
        NSNumber *number = value;
        if (IsBoolean(number)) return (bool)number.boolValue;
        if (CFNumberIsFloatType((__bridge CFNumberRef)number)) return number.doubleValue;
        return (int64_t)number.longLongValue;
    }
    if ([value isKindOfClass:NSArray.class]) {
        nlohmann::json array = nlohmann::json::array();
        for (id item in value) array.push_back(AstralJSONFromObject(item, depth + 1));
        return array;
    }
    if ([value isKindOfClass:NSDictionary.class]) {
        nlohmann::json object = nlohmann::json::object();
        for (id key in value) {
            if ([key isKindOfClass:NSString.class]) object[[key UTF8String]] = AstralJSONFromObject(value[key], depth + 1);
        }
        return object;
    }
    return nullptr;
}
