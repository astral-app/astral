#import <Foundation/Foundation.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralStartNetworkRequest(uint64_t request, NSDictionary *_Nonnull options);
void AstralCancelNetworkRequest(uint64_t request);

#ifdef __cplusplus
}
#endif
