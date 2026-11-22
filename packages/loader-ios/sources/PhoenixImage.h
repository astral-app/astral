#import <UIKit/UIKit.h>
#import <objc/runtime.h>
#import <mach-o/loader.h>

#ifdef __cplusplus
extern "C" {
#endif

UIWindow *AstralPhoenixWindow(void);
const struct mach_header *AstralPhoenixImage(void);
Method AstralPhoenixOwnedMethod(Class cls, SEL selector, const char *encoding);
BOOL AstralIsDiscordApp(void);
NSString *AstralPhoenixCompatibility(void);
NSString *AstralPhoenixUUID(void);

#ifdef __cplusplus
}
#endif
