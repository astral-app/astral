#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSData *_Nonnull AstralPluginSigningMessage(NSData *_Nonnull manifest, NSData *_Nonnull source, NSString *_Nullable directory);
BOOL AstralVerifySignature(NSData *_Nonnull message, NSString *_Nullable signature);

#ifdef __cplusplus
}
#endif
