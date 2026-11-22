#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSArray<NSDictionary *> *_Nonnull AstralThemes(void);
NSString *_Nonnull AstralActiveThemeID(void);
void AstralApplyTheme(NSString *_Nonnull themeID);
NSString *_Nonnull AstralLocalThemesDirectory(void);
NSString *_Nullable AstralImportUnboundTheme(NSData *_Nonnull json, NSString *_Nonnull fallbackName);

#ifdef __cplusplus
}
#endif
