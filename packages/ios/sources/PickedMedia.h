#import <Foundation/Foundation.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

BOOL AstralPickedMediaInfo(NSUInteger slot, int64_t *_Nonnull size, double *_Nonnull duration, NSString *_Nullable *_Nonnull type,
                           void (^_Nonnull refresh)(void));

#ifdef __cplusplus
}
#endif
