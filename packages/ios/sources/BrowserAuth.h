#import <Foundation/Foundation.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

BOOL AstralOpenPluginWebAuth(uint64_t request, NSString *_Nonnull url, NSString *_Nonnull callbackURL,
                             void (^_Nonnull completion)(uint64_t, NSString *_Nullable, NSString *_Nullable, NSString *_Nullable));
void AstralCancelPluginWebAuth(uint64_t request);

#ifdef __cplusplus
}
#endif
