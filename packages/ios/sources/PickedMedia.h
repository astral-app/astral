#import <Foundation/Foundation.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

BOOL AstralPickedMediaInfo(NSUInteger slot, int64_t *_Nonnull size, double *_Nonnull duration, NSString *_Nullable *_Nonnull type,
                           void (^_Nonnull refresh)(void));
NSString *_Nullable AstralPickedVideoURL(NSUInteger slot);
BOOL AstralIsPickedVideoURL(NSURL *_Nullable url);
NSURL *_Nullable AstralPickedVideoFile(NSURL *_Nonnull url);
NSRange AstralByteRange(NSString *_Nullable header, NSUInteger length);

#ifdef __cplusplus
}
#endif
