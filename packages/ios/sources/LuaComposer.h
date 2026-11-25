#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralInstallLuaComposer(void);
void AstralRepaintLuaComposers(void);
BOOL AstralInsertComposerText(NSString *_Nonnull text, BOOL replace);

#ifdef __cplusplus
}
#endif
