#import <UIKit/UIKit.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef NSDictionary *(*AstralFallbackModel)(void);
typedef void (*AstralFallbackAction)(NSString *key);

BOOL AstralInstallSettingsEntry(AstralFallbackModel model, AstralFallbackAction action);

#ifdef __cplusplus
}
#endif
