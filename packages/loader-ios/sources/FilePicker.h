#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralPickFile(NSArray<NSString *> *_Nonnull extensions,
                    void (^_Nonnull completion)(NSData *_Nullable data, NSString *_Nullable name));
void AstralShowAlert(NSString *_Nonnull title, NSString *_Nonnull message);

#ifdef __cplusplus
}
#endif
