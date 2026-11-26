#import "CrashReports.h"

#import <MetricKit/MetricKit.h>

#import "AstralCore.h"
#import "EnvironmentReport.h"

@interface AstralCrashSubscriber : NSObject <MXMetricManagerSubscriber>
@end

static NSData *AppImages(void)
{
    NSMutableArray<NSString *> *loaded = [NSMutableArray array];
    for (NSString *image in AstralLoadedImages()) [loaded addObject:image.lastPathComponent];
    NSDictionary *images = @{
        @"loaded" : loaded,
        @"executable" : NSBundle.mainBundle.executablePath.lastPathComponent ?: @"",
        @"frameworks" : NSBundle.mainBundle.privateFrameworksPath ?: @"",
    };
    return [NSJSONSerialization dataWithJSONObject:images options:0 error:nil] ?: [NSData data];
}

@implementation AstralCrashSubscriber
- (void)didReceiveDiagnosticPayloads:(NSArray<MXDiagnosticPayload *> *)payloads
{
    NSData *images = AppImages();
    for (MXDiagnosticPayload *payload in payloads)
        astral_ios_record_diagnostics(AstralTextOfData(payload.JSONRepresentation), AstralTextOfData(images));
}
@end

void AstralStartCrashReports(void)
{
    static AstralCrashSubscriber *subscriber;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
            subscriber = [AstralCrashSubscriber new];
            MXMetricManager *manager = MXMetricManager.sharedManager;
            [manager addSubscriber:subscriber];
            [subscriber didReceiveDiagnosticPayloads:manager.pastDiagnosticPayloads];
        });
    });
}
