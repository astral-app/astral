#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralInstallNetworkFilter(BOOL (^_Nonnull staffDevTools)(void));
NSDictionary *_Nonnull AstralNetworkFilterStats(void);
BOOL AstralInjectGatewayMessage(NSString *_Nonnull message);

#ifdef __cplusplus
}
#endif
