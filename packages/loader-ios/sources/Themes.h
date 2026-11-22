#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSArray<NSDictionary *> *_Nonnull AstralThemes(void);
NSString *_Nonnull AstralActiveThemeID(void);
void AstralApplyTheme(NSString *_Nonnull themeID);
NSString *_Nonnull AstralLocalThemesDirectory(void);

#ifdef __cplusplus
}
#endif
