#pragma once

#import <Foundation/Foundation.h>

#include <nlohmann/json.hpp>

id _Nullable AstralObjectFromJSON(const nlohmann::json &value);
nlohmann::json AstralJSONFromObject(id _Nullable value, int depth = 0);
