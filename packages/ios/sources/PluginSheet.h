#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (^AstralSheetEvent)(NSString *_Nonnull action, NSString *_Nonnull value, BOOL closed);

BOOL AstralOpenSheet(uint64_t sheet, NSDictionary *_Nonnull spec, AstralSheetEvent _Nonnull event);
void AstralUpdateSheet(uint64_t sheet, NSDictionary *_Nonnull spec);
void AstralCloseSheet(uint64_t sheet);

#ifdef __cplusplus
}
#endif
