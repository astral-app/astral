#import "EnvironmentReport.h"

#import <Security/Security.h>
#import <UIKit/UIKit.h>
#include <arpa/inet.h>
#include <dlfcn.h>
#include <errno.h>
#include <ifaddrs.h>
#include <mach-o/dyld.h>
#include <net/if.h>
#import <objc/message.h>
#include <sys/sysctl.h>
#include <unistd.h>

#import "AstralCore.h"

extern int csops(pid_t pid, unsigned int ops, void *useraddr, size_t usersize);

static const uint32_t kCodeSigningDebugged = 0x10000000;
static const unsigned int kCodeSigningOpsStatus = 0;
static const unsigned int kCodeSigningOpsEntitlementsBlob = 7;
enum { kBlobHeaderLength = 8 };
static const size_t kMaximumBlobLength = 1 << 20;
static NSString *const kHttpsProbeURL = @"https://discord.com/api/v9/gateway";
static const int64_t kHttpsCheckDelay = 20 * NSEC_PER_SEC;
static const NSTimeInterval kHttpsTimeout = 15;
static const char *const kOwnerVariable = "ASTRAL_IMAGE";
static const char *const kDuplicateVariable = "ASTRAL_DUPLICATE_IMAGE";
static NSString *const kSideStoreVPNPrefix = @"10.7.0.";
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

static NSString *SysctlText(const char *name)
{
    char value[64] = {0};
    size_t size = sizeof(value);
    return sysctlbyname(name, value, &size, NULL, 0) == 0 ? @(value) : @"";
}

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

static BOOL CodeSigningDebugged(void)
{
    uint32_t flags = 0;
    return csops(getpid(), kCodeSigningOpsStatus, &flags, sizeof(flags)) == 0 && (flags & kCodeSigningDebugged) != 0;
}

static BOOL DebuggerAttached(void)
{
    struct kinfo_proc info = {0};
    size_t size = sizeof(info);
    int name[] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()};
    return sysctl(name, 4, &info, &size, NULL, 0) == 0 && (info.kp_proc.p_flag & P_TRACED) != 0;
}

static NSData *KernelEntitlements(void)
{
    uint8_t header[kBlobHeaderLength] = {0};
    if (csops(getpid(), kCodeSigningOpsEntitlementsBlob, header, sizeof(header)) == 0)
        return [NSData dataWithBytes:header length:sizeof(header)];
    if (errno != ERANGE) return nil;
    size_t length = ((size_t)header[4] << 24) | ((size_t)header[5] << 16) | ((size_t)header[6] << 8) | (size_t)header[7];
    if (length <= sizeof(header) || length > kMaximumBlobLength) return nil;
    NSMutableData *blob = [NSMutableData dataWithLength:length];
    return csops(getpid(), kCodeSigningOpsEntitlementsBlob, blob.mutableBytes, length) == 0 ? blob : nil;
}

static BOOL HasSideStoreVPN(void)
{
    struct ifaddrs *interfaces = NULL;
    if (getifaddrs(&interfaces) != 0) return NO;
    BOOL found = NO;
    for (struct ifaddrs *entry = interfaces; entry && !found; entry = entry->ifa_next) {
        if (!entry->ifa_addr || entry->ifa_addr->sa_family != AF_INET || strncmp(entry->ifa_name, "utun", 4) != 0 ||
            !(entry->ifa_flags & IFF_UP))
            continue;
        char address[INET_ADDRSTRLEN] = {0};
        inet_ntop(AF_INET, &((struct sockaddr_in *)entry->ifa_addr)->sin_addr, address, sizeof(address));
        found = [@(address) hasPrefix:kSideStoreVPNPrefix];
    }
    freeifaddrs(interfaces);
    return found;
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

static NSString *ImagePath(void)
{
    Dl_info info = {0};
    if (!dladdr((const void *)&ImagePath, &info) || !info.dli_fname) return @"";
    return @(info.dli_fname);
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

static NSString *BootstrapRoot(void)
{
    const char *(*rootGetter)(void) = (const char *(*)(void))dlsym(RTLD_DEFAULT, "get_jbroot");
    const char *root = rootGetter ? rootGetter() : NULL;
    return root && root[0] ? @(root) : @"";
}

static BOOL RoothideJailbroken(void)
{
    bool (*getter)(void) = (bool (*)(void))dlsym(RTLD_DEFAULT, "jbclient_roothide_jailbroken");
    return getter && getter();
}

static id HookProvider(void)
{
    Dl_info provider = {0};
    Dl_info libhooker = {0};
    void *messageHook = dlsym(RTLD_DEFAULT, "MSHookMessageEx");
    void *libhookerHooks = dlsym(RTLD_DEFAULT, "LHHookFunctions");
    if (!messageHook || !dladdr(messageHook, &provider) || !provider.dli_fname) return NSNull.null;
    BOOL sameImage = libhookerHooks && dladdr(libhookerHooks, &libhooker) && libhooker.dli_fbase == provider.dli_fbase;
    return @{@"path" : @(provider.dli_fname), @"libhooker" : @(sameImage)};
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
    const char *duplicate = getenv(kDuplicateVariable);
    id channel = bundle.infoDictionary[@"RELEASE_CHANNEL"];
    NSData *entitlements = KernelEntitlements();
    return [@{
        @"bundlePath" : bundle.bundlePath ?: @"",
        @"executablePath" : bundle.executablePath ?: @"",
        @"documentsPath" : NSSearchPathForDirectoriesInDomains(NSDocumentDirectory, NSUserDomainMask, YES).firstObject ?: @"",
        @"imagePath" : ImagePath(),
        @"images" : AstralLoadedImages(),
        @"bootstrapRoot" : BootstrapRoot(),
        @"roothide" : @(RoothideJailbroken()),
        @"hookProvider" : HookProvider(),
        @"machine" : SysctlText("hw.machine"),
        @"osBuild" : SysctlText("kern.osversion"),
        @"deviceName" : DeviceName(),
        @"artworkSubtype" : @(ArtworkSubtype()),
        @"codeSigningDebugged" : @(CodeSigningDebugged()),
        @"debuggerAttached" : @(DebuggerAttached()),
        @"sideStoreVPN" : @(HasSideStoreVPN()),
        @"kernelEntitlements" : [entitlements base64EncodedStringWithOptions:0] ?: @"",
        @"receiptName" : receipt.lastPathComponent ?: (id)NSNull.null,
        @"receiptExists" : @((BOOL)(receipt && [NSFileManager.defaultManager fileExistsAtPath:receipt.path])),
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
        @"duplicateCopy" : duplicate ? @(duplicate) : @"",
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
