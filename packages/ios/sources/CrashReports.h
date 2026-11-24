#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralStartCrashReports(NSArray<NSString *> *_Nonnull (^_Nonnull runningPlugins)(void));
void AstralRecordPluginFailure(NSString *_Nonnull pluginID, NSString *_Nonnull pluginName, NSString *_Nonnull error);
void AstralRecordUnexpectedExit(NSString *_Nonnull pluginID, NSArray<NSString *> *_Nonnull running);
NSArray<NSDictionary *> *_Nonnull AstralCrashReports(void);
NSString *_Nonnull AstralCrashReportText(NSDictionary *_Nonnull report);
NSURL *_Nullable AstralCrashReportIssueURL(NSDictionary *_Nonnull report);

#ifdef __cplusplus
}
#endif
