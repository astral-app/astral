#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralInstallNetworkFilter(BOOL (^_Nonnull staffDevTools)(void));
BOOL AstralInjectGatewayMessage(NSString *_Nonnull message);
BOOL AstralDispatchLocalGatewayEvent(NSString *_Nonnull message);
void AstralRestartGatewaySession(void);

#ifdef __cplusplus
}
#endif
