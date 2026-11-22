#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralDevAppendLog(NSString *_Nonnull line);
NSString *_Nonnull AstralDevPairingCode(void);
void AstralStartDevServer(BOOL (^_Nonnull enabled)(void), void (^_Nonnull changed)(void));

#ifdef __cplusplus
}
#endif
