#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef NSDictionary *_Nonnull (^AstralSettingsPage)(NSString *_Nonnull route);
typedef NSString *_Nullable (^AstralSettingsAction)(NSString *_Nonnull route, NSString *_Nonnull key);

NSString *_Nonnull AstralPhoenixRoute(void);
NSString *_Nullable AstralInstallPhoenixSettings(NSDictionary *_Nonnull category, AstralSettingsPage _Nonnull page,
                                                 AstralSettingsAction _Nonnull action);
void AstralRefreshPhoenixSettings(void);

#ifdef __cplusplus
}
#endif
