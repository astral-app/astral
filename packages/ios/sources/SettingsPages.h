#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSDictionary *_Nonnull AstralSettingsCategory(void);
NSDictionary *_Nonnull AstralSettingsPageModel(NSString *_Nonnull route);
NSString *_Nullable AstralSettingsPageAction(NSString *_Nonnull route, NSString *_Nonnull key);

#ifdef __cplusplus
}
#endif
