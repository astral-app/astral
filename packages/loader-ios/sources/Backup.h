#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralExportBackup(void);
void AstralImportBackup(void (^_Nonnull changed)(void));
NSString *_Nullable AstralBackupTOML(void);
NSString *_Nullable AstralApplyBackup(NSData *_Nonnull data, void (^_Nonnull changed)(void));

#ifdef __cplusplus
}
#endif
