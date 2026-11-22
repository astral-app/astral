#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSDictionary<NSString *, NSString *> *_Nullable AstralSignedFiles(NSData *_Nullable signatureFile);
BOOL AstralVerifyPluginDirectory(NSString *_Nonnull directory, NSString *_Nonnull main);
BOOL AstralVerifySignature(NSData *_Nonnull message, NSString *_Nullable signature);

#ifdef __cplusplus
}
#endif
