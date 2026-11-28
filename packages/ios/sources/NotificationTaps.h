#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

extern NSString *const _Nonnull AstralNotificationLinkKey;
extern NSString *const _Nonnull AstralNotificationReplyKey;

void AstralRouteNotificationTaps(void);
void AstralWithReplyCategory(NSDictionary *_Nonnull reply, void (^_Nonnull ready)(NSString *_Nonnull category));

#ifdef __cplusplus
}
#endif
