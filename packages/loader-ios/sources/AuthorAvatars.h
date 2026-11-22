#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSString *_Nonnull AstralAuthorAvatarURL(NSString *_Nonnull userID, NSUInteger index);
void AstralAuthorAvatarsOnChange(void (^_Nonnull changed)(void));

#ifdef __cplusplus
}
#endif
