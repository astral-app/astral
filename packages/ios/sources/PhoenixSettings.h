#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef NSDictionary *_Nonnull (^AstralSettingsPage)(NSString *_Nonnull route);
typedef NSString *_Nullable (^AstralSettingsAction)(NSString *_Nonnull route, NSString *_Nonnull key);

NSString *_Nonnull AstralPhoenixRoute(void);
NSString *_Nullable AstralStartPhoenixRoute(void);
void AstralStartPhoenixRouteWhenLoaded(void (^_Nonnull started)(void));
NSDictionary<NSString *, NSNumber *> *_Nonnull AstralPhoenixTimings(void);
void AstralPhoenixRecordMoment(NSString *_Nonnull key);
void AstralPhoenixPluginsReady(void);
void AstralOnPhoenixRouteChange(void (^_Nonnull changed)(void));
NSString *_Nullable AstralInstallPhoenixSettings(NSDictionary *_Nonnull category, AstralSettingsPage _Nonnull page,
                                                 AstralSettingsAction _Nonnull action);
void AstralRefreshPhoenixSettings(void);
void AstralPhoenixRefreshDecorations(void);
void AstralPhoenixDeliver(const uint8_t *_Nullable bytes, size_t length);
void AstralPhoenixLockWire(void);
void AstralPhoenixUnlockWire(void);

#ifdef __cplusplus
}
#endif
