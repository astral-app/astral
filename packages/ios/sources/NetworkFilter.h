#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralInstallNetworkFilter(BOOL (^_Nonnull staffDevTools)(void));
BOOL AstralInjectGatewayMessage(NSString *_Nonnull message);
BOOL AstralDispatchLocalGatewayEvent(NSString *_Nonnull message);
void AstralRestartGatewaySession(void);
void AstralSendDiscordRequest(uint64_t identifier, NSString *_Nonnull method, NSString *_Nonnull path, NSString *_Nonnull body);

#ifdef __cplusplus
}
#endif
