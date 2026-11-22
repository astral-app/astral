#import <Foundation/Foundation.h>

#ifdef __cplusplus
namespace astral {
class SettingsStore;
}
astral::SettingsStore *_Nonnull AstralSettings(void);
extern "C" {
#endif

NSString *_Nonnull AstralStateDirectory(void);
NSString *_Nonnull AstralResourceDirectory(void);
NSString *_Nonnull AstralVersion(void);

#ifdef __cplusplus
}
#endif
