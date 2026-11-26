#import "EnvironmentReport.h"

#import <Security/Security.h>
#import <UIKit/UIKit.h>
#include <dlfcn.h>
#include <mach-o/dyld.h>
#import <objc/message.h>

#import "AstralCore.h"

static NSString *const kHttpsProbeURL = @"https://discord.com/api/v9/gateway";
static const int64_t kHttpsCheckDelay = 20 * NSEC_PER_SEC;
static const NSTimeInterval kHttpsTimeout = 15;
static const char *const kOwnerVariable = "ASTRAL_IMAGE";
static const char *const kDuplicateVariable = "ASTRAL_DUPLICATE_IMAGE";
static const NSInteger kIPadFamily = 2;

__attribute__((constructor(101))) static void ClaimProcess(void)
{
    Dl_info info = {0};
    if (!dladdr((const void *)&ClaimProcess, &info) || !info.dli_fname) return;
    const char *owner = getenv(kOwnerVariable);
    if (!owner) {
        setenv(kOwnerVariable, info.dli_fname, 1);
        return;
    }
    if (strcmp(owner, info.dli_fname) != 0) setenv(kDuplicateVariable, info.dli_fname, 1);
}

static void RecordHttpsCheck(NSDictionary *check)
{
    NSData *json = [NSJSONSerialization dataWithJSONObject:check options:0 error:nil];
    if (json) astral_ios_record_https_check(AstralTextOfData(json));
}

@interface AstralHttpsChainRecorder : NSObject <NSURLSessionDelegate>
@property (atomic) BOOL completed;
@end

@implementation AstralHttpsChainRecorder

- (void)URLSession:(__unused NSURLSession *)session
    didReceiveChallenge:(NSURLAuthenticationChallenge *)challenge
      completionHandler:(void (^)(NSURLSessionAuthChallengeDisposition, NSURLCredential *))completionHandler
{
    SecTrustRef trust = challenge.protectionSpace.serverTrust;
    if (trust) {
        NSMutableArray<NSString *> *issuers = [NSMutableArray array];
        CFArrayRef chain = SecTrustCopyCertificateChain(trust);
        CFIndex count = chain ? CFArrayGetCount(chain) : 0;
        for (CFIndex index = 1; index < count; index++) {
            SecCertificateRef certificate = (SecCertificateRef)CFArrayGetValueAtIndex(chain, index);
            NSString *summary = (__bridge_transfer NSString *)SecCertificateCopySubjectSummary(certificate);
            if (summary) [issuers addObject:summary];
        }
        if (chain) CFRelease(chain);
        self.completed = YES;
        RecordHttpsCheck(@{@"completed" : @YES, @"issuers" : issuers});
    }
    completionHandler(NSURLSessionAuthChallengePerformDefaultHandling, nil);
}

@end

static CFTypeRef CopyGestaltAnswer(CFStringRef key)
{
    static void *library;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        library = dlopen("/usr/lib/libMobileGestalt.dylib", RTLD_LAZY);
    });
    CFTypeRef (*copyAnswer)(CFStringRef) = library ? (CFTypeRef (*)(CFStringRef))dlsym(library, "MGCopyAnswer") : NULL;
    return copyAnswer ? copyAnswer(key) : NULL;
}

static NSString *DeviceName(void)
{
    CFTypeRef answer = CopyGestaltAnswer(CFSTR("PhysicalHardwareNameString"));
    NSString *name = answer && CFGetTypeID(answer) == CFStringGetTypeID() ? [(__bridge NSString *)answer copy] : nil;
    if (answer) CFRelease(answer);
    return name ?: @"";
}

static long ArtworkSubtype(void)
{
    CFTypeRef answer = CopyGestaltAnswer(CFSTR("ArtworkDeviceSubType"));
    long subtype = 0;
    if (answer && CFGetTypeID(answer) == CFNumberGetTypeID()) CFNumberGetValue((CFNumberRef)answer, kCFNumberLongType, &subtype);
    if (answer) CFRelease(answer);
    return subtype;
}

static NSInteger RefreshRate(void)
{
    static NSInteger cached;
    if (NSThread.isMainThread)
        cached = UIScreen.mainScreen.maximumFramesPerSecond;
    else if (cached == 0)
        dispatch_async(dispatch_get_main_queue(), ^{
            cached = UIScreen.mainScreen.maximumFramesPerSecond;
        });
    return cached;
}

static BOOL IsiOSAppOnVision(void)
{
    NSProcessInfo *process = NSProcessInfo.processInfo;
    SEL selector = NSSelectorFromString(@"isiOSAppOnVision");
    return [process respondsToSelector:selector] && ((BOOL (*)(id, SEL))objc_msgSend)(process, selector);
}

static BOOL SupportsIPad(void)
{
    NSArray *families = NSBundle.mainBundle.infoDictionary[@"UIDeviceFamily"];
    return [families isKindOfClass:NSArray.class] && [families containsObject:@(kIPadFamily)];
}

static BOOL IsManagedApp(void)
{
    id configuration = [NSUserDefaults.standardUserDefaults objectForKey:@"com.apple.configuration.managed"];
    return [configuration isKindOfClass:NSDictionary.class] && [configuration count] > 0;
}

static NSDictionary *LocaleFacts(void)
{
    NSString *zero = [NSNumberFormatter localizedStringFromNumber:@0 numberStyle:NSNumberFormatterDecimalStyle];
    NSString *language = NSLocale.preferredLanguages.firstObject;
    return @{
        @"calendar" : NSLocale.currentLocale.calendarIdentifier ?: @"",
        @"nonLatinDigits" : @((BOOL)(zero.length > 0 && ![zero isEqualToString:@"0"])),
        @"rightToLeft" : @((BOOL)(language && [NSLocale characterDirectionForLanguage:language] == NSLocaleLanguageDirectionRightToLeft)),
    };
}

static NSArray<NSString *> *StringsAt(NSArray *entries, NSString *key)
{
    NSMutableArray<NSString *> *values = [NSMutableArray array];
    for (id entry in [entries isKindOfClass:NSArray.class] ? entries : @[]) {
        id value = [entry isKindOfClass:NSDictionary.class] ? entry[key] : nil;
        NSArray *items = [value isKindOfClass:NSArray.class] ? value : (value ? @[ value ] : @[]);
        for (id item in items)
            if ([item isKindOfClass:NSString.class]) [values addObject:item];
    }
    return values;
}

NSArray<NSString *> *AstralLoadedImages(void)
{
    NSMutableArray<NSString *> *images = [NSMutableArray array];
    for (uint32_t index = 0; index < _dyld_image_count(); index++) {
        const char *name = _dyld_get_image_name(index);
        if (name) [images addObject:@(name)];
    }
    return images;
}

static BOOL HasLiveContainerClasses(void)
{
    return NSClassFromString(@"LCSharedUtils") || NSClassFromString(@"LCAppInfo");
}

NSMutableDictionary<NSString *, id> *AstralEnvironmentFacts(void)
{
    NSBundle *bundle = NSBundle.mainBundle;
    NSURL *receipt = bundle.appStoreReceiptURL;
    UIDevice *device = UIDevice.currentDevice;
    id channel = bundle.infoDictionary[@"RELEASE_CHANNEL"];
    return [@{
        @"bundlePath" : bundle.bundlePath ?: @"",
        @"executablePath" : bundle.executablePath ?: @"",
        @"documentsPath" : NSSearchPathForDirectoriesInDomains(NSDocumentDirectory, NSUserDomainMask, YES).firstObject ?: @"",
        @"deviceName" : DeviceName(),
        @"artworkSubtype" : @(ArtworkSubtype()),
        @"receiptName" : receipt.lastPathComponent ?: (id)NSNull.null,
        @"receiptPath" : receipt.path ?: @"",
        @"liveContainerClasses" : @(HasLiveContainerClasses()),
        @"urlSchemes" : StringsAt([bundle objectForInfoDictionaryKey:@"CFBundleURLTypes"], @"CFBundleURLSchemes"),
        @"exportedTypes" : StringsAt([bundle objectForInfoDictionaryKey:@"UTExportedTypeDeclarations"], @"UTTypeIdentifier"),
        @"altBundleIdentifier" : @((BOOL)([bundle objectForInfoDictionaryKey:@"ALTBundleIdentifier"] != nil)),
        @"altDeviceIdentifier" : @((BOOL)([bundle objectForInfoDictionaryKey:@"ALTDeviceID"] != nil)),
        @"releaseChannel" : [channel isKindOfClass:NSString.class] ? channel : NSNull.null,
        @"lockdownMode" : @([NSUserDefaults.standardUserDefaults boolForKey:@"LDMGlobalEnabled"]),
        @"iPhoneAppOnIPad" : @((BOOL)(device.userInterfaceIdiom == UIUserInterfaceIdiomPad && !SupportsIPad())),
        @"iOSAppOnMac" : @(NSProcessInfo.processInfo.isiOSAppOnMac),
        @"iOSAppOnVision" : @(IsiOSAppOnVision()),
        @"managedApp" : @(IsManagedApp()),
        @"refreshRate" : @(RefreshRate()),
        @"boldText" : @(UIAccessibilityIsBoldTextEnabled()),
        @"locale" : LocaleFacts(),
    } mutableCopy];
}

void AstralStartHttpsCheck(void)
{
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, kHttpsCheckDelay), dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        NSURLSessionConfiguration *configuration = NSURLSessionConfiguration.ephemeralSessionConfiguration;
        configuration.timeoutIntervalForRequest = kHttpsTimeout;
        AstralHttpsChainRecorder *recorder = [AstralHttpsChainRecorder new];
        NSURLSession *session = [NSURLSession sessionWithConfiguration:configuration delegate:recorder delegateQueue:nil];
        NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:[NSURL URLWithString:kHttpsProbeURL]];
        request.HTTPMethod = @"HEAD";
        [[session dataTaskWithRequest:request
                    completionHandler:^(__unused NSData *data, __unused NSURLResponse *response, NSError *error) {
                        if (error && !recorder.completed)
                            RecordHttpsCheck(@{@"failure" : [NSString stringWithFormat:@"%@ %ld", error.domain, (long)error.code]});
                        [session finishTasksAndInvalidate];
                    }] resume];
    });
}
