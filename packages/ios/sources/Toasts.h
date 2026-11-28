#import <Foundation/Foundation.h>

typedef void (^AstralToastEvent)(NSString *_Nonnull action, NSString *_Nonnull value, BOOL closed);

void AstralShowToast(uint64_t toast, NSDictionary<NSString *, id> *_Nonnull spec, AstralToastEvent _Nullable event);
void AstralCloseToast(uint64_t toast);
