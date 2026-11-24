#import <Foundation/Foundation.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralPickFile(NSArray<NSString *> *_Nonnull extensions,
                    void (^_Nonnull completion)(NSData *_Nullable data, NSString *_Nullable name));
BOOL AstralPickPluginFile(uint64_t request, NSArray<NSString *> *_Nonnull extensions,
                          void (^_Nonnull completion)(uint64_t request, NSData *_Nullable data, NSString *_Nullable name,
                                                      NSString *_Nullable error));
void AstralCancelPluginFile(uint64_t request);
void AstralShowAlert(NSString *_Nonnull title, NSString *_Nonnull message);
void AstralShowToast(NSString *_Nonnull title, NSString *_Nonnull message);
void AstralEditText(NSString *_Nonnull title, NSString *_Nonnull value, NSUInteger maximumLength,
                    void (^_Nonnull completion)(NSString *_Nullable value));

#ifdef __cplusplus
}
#endif
