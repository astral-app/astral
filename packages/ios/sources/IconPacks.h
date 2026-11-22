#import <UIKit/UIKit.h>

#ifdef __cplusplus
extern "C" {
#endif

NSArray<NSDictionary *> *_Nonnull AstralIconPacks(void);
NSString *_Nonnull AstralActiveIconPackID(void);
void AstralSelectIconPack(NSString *_Nonnull identifier);
UIImage *_Nullable AstralIconPackImage(NSString *_Nonnull name);

#ifdef __cplusplus
}
#endif
