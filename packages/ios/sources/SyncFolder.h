#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSString *_Nullable AstralSyncFolderName(void);
void AstralChooseSyncFolder(void);
void AstralForgetSyncFolder(void);
NSString *_Nullable AstralReadSyncFile(void);
BOOL AstralWriteSyncFile(NSString *_Nonnull contents);

#ifdef __cplusplus
}
#endif
