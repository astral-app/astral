#import <Foundation/Foundation.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralFetchPickedMedia(NSUInteger limit);
BOOL AstralIsPickedVideoURL(NSURL *_Nullable url);
NSURL *_Nullable AstralPickedVideoFile(NSURL *_Nonnull url);
NSRange AstralByteRange(NSString *_Nullable header, NSUInteger length);

#ifdef __cplusplus
}
#endif
