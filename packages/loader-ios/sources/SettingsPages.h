#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    BOOL recovering;
    NSString *_Nonnull discordVersion;
    NSString *_Nonnull compatibility;
} AstralPageContext;

NSDictionary *_Nonnull AstralSettingsCategory(void);
NSDictionary *_Nonnull AstralSettingsPageModel(NSString *_Nonnull route, AstralPageContext context);
NSString *_Nullable AstralSettingsPageAction(NSString *_Nonnull route, NSString *_Nonnull key, void (^_Nonnull changed)(void));

#ifdef __cplusplus
}
#endif
