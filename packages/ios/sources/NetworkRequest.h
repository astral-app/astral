#import <Foundation/Foundation.h>

#ifdef __cplusplus
#include <cstdint>
#include <nlohmann/json.hpp>

namespace astral {
void startNetworkRequest(uint64_t request, const nlohmann::json &options);
void cancelNetworkRequest(uint64_t request);
}
#endif
