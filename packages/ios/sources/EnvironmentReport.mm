#import "EnvironmentReport.h"

#import <Security/Security.h>
#import <UIKit/UIKit.h>
#include <arpa/inet.h>
#include <atomic>
#include <cerrno>
#include <cstring>
#include <dlfcn.h>
#include <ifaddrs.h>
#include <mach-o/dyld.h>
#include <net/if.h>
#import <objc/message.h>
#include <set>
#include <sys/sysctl.h>
#include <unistd.h>

#include "AstralBinaryAnatomy.h"
#include "AstralCodeSignature.h"
#include "AstralEnvironment.h"
#include "AstralEnvironmentReport.h"
#include "AstralLeftoverMods.h"
#include "AstralNetworkStack.h"

namespace environment = astral::environment;
namespace report = astral::environment_report;

extern "C" int csops(pid_t pid, unsigned int ops, void *useraddr, size_t usersize);

static constexpr uint32_t kCodeSigningDebugged = 0x10000000;
static constexpr unsigned int kCodeSigningOpsEntitlementsBlob = 7;
static NSString *const kHttpsProbeURL = @"https://discord.com/api/v9/gateway";
static const int64_t kHttpsCheckDelay = 20 * NSEC_PER_SEC;
static const char *const kOwnerVariable = "ASTRAL_IMAGE";
static const char *const kDuplicateVariable = "ASTRAL_DUPLICATE_IMAGE";
static const std::set<long> kDynamicIslandSubtypes = {2556, 2622, 2796, 2868};
static NSString *const kSideStoreVPNPrefix = @"10.7.0.";

static std::string Plain(NSString *text)
{
    return text.UTF8String ?: "";
}

__attribute__((constructor(101))) static void ClaimProcess(void)
{
    Dl_info info = {};
    if (!dladdr((const void *)&ClaimProcess, &info) || !info.dli_fname) return;
    const char *owner = getenv(kOwnerVariable);
    if (!owner) {
        setenv(kOwnerVariable, info.dli_fname, 1);
        return;
    }
    if (strcmp(owner, info.dli_fname) != 0) setenv(kDuplicateVariable, info.dli_fname, 1);
}

@interface AstralHttpsChainRecorder : NSObject <NSURLSessionDelegate>
@end

@implementation AstralHttpsChainRecorder

- (void)URLSession:(__unused NSURLSession *)session
    didReceiveChallenge:(NSURLAuthenticationChallenge *)challenge
      completionHandler:(void (^)(NSURLSessionAuthChallengeDisposition, NSURLCredential *))completionHandler
{
    SecTrustRef trust = challenge.protectionSpace.serverTrust;
    if (trust) {
        report::HttpsCheck check;
        check.completed = true;
        CFArrayRef chain = SecTrustCopyCertificateChain(trust);
        CFIndex count = chain ? CFArrayGetCount(chain) : 0;
        for (CFIndex index = 1; index < count; index++) {
            auto certificate = (SecCertificateRef)CFArrayGetValueAtIndex(chain, index);
            CFStringRef summary = SecCertificateCopySubjectSummary(certificate);
            if (!summary) continue;
            check.issuers.push_back(Plain((__bridge NSString *)summary));
            CFRelease(summary);
        }
        if (chain) CFRelease(chain);
        report::recordHttpsCheck(std::move(check));
    }
    completionHandler(NSURLSessionAuthChallengePerformDefaultHandling, nil);
}

@end

static NSString *BootstrapRootOfImage(const char *path)
{
    if (!path) return nil;
    NSString *image = [@(path) stringByResolvingSymlinksInPath];
    std::optional<std::string> root = environment::bootstrapRootOfImage(image.UTF8String);
    return root ? @(root->c_str()) : nil;
}

static NSString *ActiveBootstrapRoot(void)
{
    auto rootGetter = reinterpret_cast<const char *(*)()>(dlsym(RTLD_DEFAULT, "get_jbroot"));
    const char *root = rootGetter ? rootGetter() : nullptr;
    if (root && root[0]) return [@(root) stringByResolvingSymlinksInPath];
    Dl_info info{};
    if (dladdr(reinterpret_cast<const void *>(&ActiveBootstrapRoot), &info)) {
        NSString *own = BootstrapRootOfImage(info.dli_fname);
        if (own) return own;
    }
    for (uint32_t index = 0; index < _dyld_image_count(); index++) {
        NSString *injected = BootstrapRootOfImage(_dyld_get_image_name(index));
        if (injected) return injected;
    }
    return nil;
}

static std::string DetectedJailbreak(void)
{
    NSString *root = ActiveBootstrapRoot();
    if (!root) return "";
    std::vector<std::string> markers;
    for (NSString *marker in @[
             @".installed_relaxin", @".installed_nekojb", @".installed_nathanlr", @".palecursus_strapped", @".installed_palera1n",
             @".installed_dopamine", @".xia0o0o0o_jb_installed", @"basebins"
         ]) {
        if ([NSFileManager.defaultManager fileExistsAtPath:[root stringByAppendingPathComponent:marker]])
            markers.emplace_back(marker.UTF8String);
    }
    std::vector<std::string> images;
    for (uint32_t index = 0; index < _dyld_image_count(); index++) {
        const char *name = _dyld_get_image_name(index);
        if (name) images.emplace_back([[@(name) stringByResolvingSymlinksInPath] UTF8String]);
    }
    auto roothideGetter = reinterpret_cast<bool (*)()>(dlsym(RTLD_DEFAULT, "jbclient_roothide_jailbroken"));
    std::string name = environment::jailbreakName(root.UTF8String, markers, images, roothideGetter && roothideGetter());
    if (name != "Dopamine") return name;
    NSString *version = [NSString stringWithContentsOfFile:[root stringByAppendingPathComponent:@"basebin/.version"]
                                                  encoding:NSUTF8StringEncoding
                                                     error:nil];
    NSString *trimmed = [version stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    return environment::jailbreakLabel(name, trimmed ? trimmed.UTF8String : "");
}

static std::string SysctlText(const char *name)
{
    char value[64] = {};
    size_t size = sizeof(value);
    return sysctlbyname(name, value, &size, nullptr, 0) == 0 ? std::string(value) : "";
}

static CFTypeRef CopyGestaltAnswer(CFStringRef key)
{
    using CopyAnswer = CFTypeRef (*)(CFStringRef);
    static void *library = dlopen("/usr/lib/libMobileGestalt.dylib", RTLD_LAZY);
    auto copyAnswer = library ? (CopyAnswer)dlsym(library, "MGCopyAnswer") : nullptr;
    return copyAnswer ? copyAnswer(key) : nullptr;
}

static NSString *DeviceName(void)
{
    CFTypeRef answer = CopyGestaltAnswer(CFSTR("PhysicalHardwareNameString"));
    NSString *name = answer && CFGetTypeID(answer) == CFStringGetTypeID() ? [(__bridge NSString *)answer copy] : nil;
    if (answer) CFRelease(answer);
    return name.length ? name : @(SysctlText("hw.machine").c_str());
}

static long ArtworkSubtype(void)
{
    CFTypeRef answer = CopyGestaltAnswer(CFSTR("ArtworkDeviceSubType"));
    long subtype = 0;
    if (answer && CFGetTypeID(answer) == CFNumberGetTypeID()) CFNumberGetValue((CFNumberRef)answer, kCFNumberLongType, &subtype);
    if (answer) CFRelease(answer);
    return subtype;
}

static std::string ModelEdit(void)
{
    std::string machine = SysctlText("hw.machine");
    if (machine.rfind("iPhone", 0) != 0 || !kDynamicIslandSubtypes.contains(ArtworkSubtype())) return "";
    int major = std::atoi(machine.c_str() + 6);
    return major >= 15 && machine != "iPhone14,6" ? "" : "Dynamic Island added by a MobileGestalt edit";
}

static bool CodeSigningDebugged(void)
{
    uint32_t flags = 0;
    return csops(getpid(), 0, &flags, sizeof(flags)) == 0 && (flags & kCodeSigningDebugged) != 0;
}

static bool DebuggerAttached(void)
{
    struct kinfo_proc info = {};
    size_t size = sizeof(info);
    int name[] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()};
    return sysctl(name, 4, &info, &size, nullptr, 0) == 0 && (info.kp_proc.p_flag & P_TRACED) != 0;
}

static bool BundleContainsSuffix(NSString *suffix)
{
    for (NSString *name in [NSFileManager.defaultManager contentsOfDirectoryAtPath:NSBundle.mainBundle.bundlePath error:nil])
        if ([name hasSuffix:suffix]) return true;
    return false;
}

static bool HasAppSyncUnified(void)
{
    for (NSString *root in @[ @"", @"/var/jb" ]) {
        NSString *path = [root stringByAppendingString:@"/Library/MobileSubstrate/DynamicLibraries/AppSyncUnified-installd.dylib"];
        if ([NSFileManager.defaultManager fileExistsAtPath:path]) return true;
    }
    return false;
}

static std::string JailbreakVersion(void)
{
    NSString *version = [NSString stringWithContentsOfFile:@"/var/jb/basebin/.version" encoding:NSUTF8StringEncoding error:nil];
    NSString *trimmed = [version stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    bool dopamine = [NSFileManager.defaultManager fileExistsAtPath:@"/var/jb/.installed_dopamine"];
    return dopamine && trimmed.length ? "Dopamine " + Plain(trimmed) : "";
}

static bool HasSideStoreVPN(void)
{
    struct ifaddrs *interfaces = nullptr;
    if (getifaddrs(&interfaces) != 0) return false;
    bool found = false;
    for (struct ifaddrs *entry = interfaces; entry && !found; entry = entry->ifa_next) {
        if (!entry->ifa_addr || entry->ifa_addr->sa_family != AF_INET || strncmp(entry->ifa_name, "utun", 4) != 0 ||
            !(entry->ifa_flags & IFF_UP))
            continue;
        char address[INET_ADDRSTRLEN] = {};
        inet_ntop(AF_INET, &((struct sockaddr_in *)entry->ifa_addr)->sin_addr, address, sizeof(address));
        found = [@(address) hasPrefix:kSideStoreVPNPrefix];
    }
    freeifaddrs(interfaces);
    return found;
}

static report::LocaleFacts LocaleFacts(void)
{
    report::LocaleFacts facts;
    facts.calendar = Plain(NSLocale.currentLocale.calendarIdentifier ?: @"");
    NSString *zero = [NSNumberFormatter localizedStringFromNumber:@0 numberStyle:NSNumberFormatterDecimalStyle];
    facts.nonLatinDigits = zero.length > 0 && ![zero isEqualToString:@"0"];
    NSString *language = NSLocale.preferredLanguages.firstObject;
    facts.rightToLeft = language && [NSLocale characterDirectionForLanguage:language] == NSLocaleLanguageDirectionRightToLeft;
    return facts;
}

static int RefreshRate(void)
{
    static std::atomic_int cached{0};
    if (NSThread.isMainThread)
        cached = (int)UIScreen.mainScreen.maximumFramesPerSecond;
    else if (cached == 0)
        dispatch_async(dispatch_get_main_queue(), ^{
            cached = (int)UIScreen.mainScreen.maximumFramesPerSecond;
        });
    return cached;
}

static bool IsiOSAppOnVision(void)
{
    NSProcessInfo *process = NSProcessInfo.processInfo;
    SEL selector = NSSelectorFromString(@"isiOSAppOnVision");
    return [process respondsToSelector:selector] && ((BOOL (*)(id, SEL))objc_msgSend)(process, selector);
}

static bool HasImageContaining(const char *fragment)
{
    for (uint32_t index = 0; index < _dyld_image_count(); index++) {
        const char *name = _dyld_get_image_name(index);
        if (name && std::strstr(name, fragment)) return true;
    }
    return false;
}

static bool SupportsIPad(void)
{
    NSArray *families = NSBundle.mainBundle.infoDictionary[@"UIDeviceFamily"];
    return [families isKindOfClass:NSArray.class] && [families containsObject:@2];
}

static bool IsManagedApp(void)
{
    id configuration = [NSUserDefaults.standardUserDefaults objectForKey:@"com.apple.configuration.managed"];
    return [configuration isKindOfClass:NSDictionary.class] && [configuration count] > 0;
}

static report::AppleDeviceFacts DeviceFacts(void)
{
    report::AppleDeviceFacts facts;
    facts.lockdownMode = [NSUserDefaults.standardUserDefaults boolForKey:@"LDMGlobalEnabled"];
    facts.iPhoneAppOnIPad = UIDevice.currentDevice.userInterfaceIdiom == UIUserInterfaceIdiomPad && !SupportsIPad();
    facts.iOSAppOnMac = NSProcessInfo.processInfo.isiOSAppOnMac;
    facts.playCover = HasImageContaining("/PlayTools.framework/");
    facts.managedApp = IsManagedApp();
    facts.jitEnabled = DetectedJailbreak().empty() && CodeSigningDebugged();
    facts.debuggerAttached = DebuggerAttached();
    facts.trollFools = BundleContainsSuffix(@".troll-fools.bak");
    facts.appSync = HasAppSyncUnified();
    facts.modelEdit = ModelEdit();
    facts.jailbreakVersion = JailbreakVersion();
    facts.iOSAppOnVision = IsiOSAppOnVision();
    facts.sideStoreVPN = HasSideStoreVPN();
    facts.liveContainerMultitask = [NSBundle.mainBundle.executablePath containsString:@"LiveProcess"];
    facts.refreshRate = RefreshRate();
    facts.boldText = UIAccessibilityIsBoldTextEnabled();
    facts.locale = LocaleFacts();
    const char *duplicate = getenv(kDuplicateVariable);
    facts.duplicateCopy = duplicate ? duplicate : "";
    return facts;
}

static NSDictionary *EntitlementsPlist(const std::optional<std::string> &plist)
{
    if (!plist || plist->empty()) return nil;
    NSData *data = [NSData dataWithBytes:plist->data() length:plist->size()];
    NSDictionary *entitlements = [NSPropertyListSerialization propertyListWithData:data options:0 format:nil error:nil];
    return [entitlements isKindOfClass:NSDictionary.class] && entitlements.count ? entitlements : nil;
}

static std::string KernelEntitlementsBlob(void)
{
    std::string header(8, '\0');
    if (csops(getpid(), kCodeSigningOpsEntitlementsBlob, header.data(), header.size()) == 0) return header;
    if (errno != ERANGE) return "";
    const auto *bytes = reinterpret_cast<const uint8_t *>(header.data());
    const size_t length = (size_t(bytes[4]) << 24) | (size_t(bytes[5]) << 16) | (size_t(bytes[6]) << 8) | size_t(bytes[7]);
    if (length <= header.size() || length > 1024 * 1024) return "";
    std::string blob(length, '\0');
    return csops(getpid(), kCodeSigningOpsEntitlementsBlob, blob.data(), blob.size()) == 0 ? blob : "";
}

static NSDictionary *SignatureInfo(void)
{
    if (NSDictionary *entitlements = EntitlementsPlist(astral::codesign::entitlementsFromBlob(KernelEntitlementsBlob())))
        return @{@"entitlements" : entitlements, @"source" : @"kernel"};
    NSString *executable = NSBundle.mainBundle.executablePath;
    for (const std::string &blob : astral::codesign::entitlementBlobs(Plain(executable)))
        if (NSDictionary *entitlements = EntitlementsPlist(blob)) return @{@"entitlements" : entitlements, @"source" : executable};
    return @{};
}

static std::vector<std::string> StringsAt(NSArray *entries, NSString *key)
{
    std::vector<std::string> values;
    for (id entry in [entries isKindOfClass:NSArray.class] ? entries : @[]) {
        id value = [entry isKindOfClass:NSDictionary.class] ? entry[key] : nil;
        NSArray *items = [value isKindOfClass:NSArray.class] ? value : (value ? @[ value ] : @[]);
        for (id item in items)
            if ([item isKindOfClass:NSString.class]) values.push_back(Plain(item));
    }
    return values;
}

static environment::SideloaderMarkers SideloaderMarkers(NSBundle *bundle)
{
    environment::SideloaderMarkers markers;
    markers.urlSchemes = StringsAt([bundle objectForInfoDictionaryKey:@"CFBundleURLTypes"], @"CFBundleURLSchemes");
    markers.exportedTypes = StringsAt([bundle objectForInfoDictionaryKey:@"UTExportedTypeDeclarations"], @"UTTypeIdentifier");
    markers.altBundleIdentifier = [bundle objectForInfoDictionaryKey:@"ALTBundleIdentifier"] != nil;
    markers.altDeviceIdentifier = [bundle objectForInfoDictionaryKey:@"ALTDeviceID"] != nil;
    return markers;
}

static void ReadProvisioningProfile(environment::SigningFacts &facts)
{
    NSString *path = [NSBundle.mainBundle pathForResource:@"embedded" ofType:@"mobileprovision"];
    NSData *data = path ? [NSData dataWithContentsOfFile:path] : nil;
    if (!data) return;
    auto plist = environment::provisioningProfilePlist(std::string(static_cast<const char *>(data.bytes), data.length));
    if (!plist) return;
    NSDictionary *profile = [NSPropertyListSerialization propertyListWithData:[NSData dataWithBytes:plist->data() length:plist->size()]
                                                                      options:0
                                                                       format:nil
                                                                        error:nil];
    if (![profile isKindOfClass:NSDictionary.class]) return;
    facts.hasProfile = true;
    facts.provisionsAllDevices = [profile[@"ProvisionsAllDevices"] boolValue];
    if ([profile[@"TeamName"] isKindOfClass:NSString.class]) facts.teamName = Plain(profile[@"TeamName"]);
    id devices = profile[@"ProvisionedDevices"];
    facts.listsDevices = [devices isKindOfClass:NSArray.class] && [devices count] > 0;
    if ([profile[@"ExpirationDate"] isKindOfClass:NSDate.class]) {
        NSISO8601DateFormatter *formatter = [NSISO8601DateFormatter new];
        formatter.formatOptions = NSISO8601DateFormatWithFullDate;
        facts.profileExpiry = Plain([formatter stringFromDate:profile[@"ExpirationDate"]]);
    }
}

static std::string InstallDetails(NSDictionary *signature)
{
    NSDictionary *entitlements = signature[@"entitlements"];
    environment::SigningFacts facts;
    id team = entitlements[@"com.apple.developer.team-identifier"];
    if ([team isKindOfClass:NSString.class]) facts.teamIdentifier = Plain(team);
    facts.getTaskAllow = [entitlements[@"get-task-allow"] isKindOfClass:NSNumber.class] && [entitlements[@"get-task-allow"] boolValue];
    NSString *source = signature[@"source"];
    if (source.length) facts.entitlementSource = [source isEqualToString:@"kernel"] ? "kernel" : "executable";
    ReadProvisioningProfile(facts);
    facts.sideloader = environment::sideloaderName(SideloaderMarkers(NSBundle.mainBundle));
    return environment::signingDetails(facts);
}

static environment::Receipt Receipt(void)
{
    NSURL *receipt = NSBundle.mainBundle.appStoreReceiptURL;
    if (!receipt) return environment::Receipt::None;
    return environment::receiptKind(Plain(receipt.lastPathComponent), [NSFileManager.defaultManager fileExistsAtPath:receipt.path]);
}

static bool IsLiveContainerApp(void)
{
    if (NSClassFromString(@"LCSharedUtils") || NSClassFromString(@"LCAppInfo")) return true;
    char path[PATH_MAX];
    uint32_t size = sizeof(path);
    NSString *bundle = NSBundle.mainBundle.bundlePath.stringByResolvingSymlinksInPath.stringByStandardizingPath;
    if (_NSGetExecutablePath(path, &size) != 0 || !bundle.length) return false;
    NSString *executable = [NSString stringWithUTF8String:path].stringByResolvingSymlinksInPath.stringByStandardizingPath;
    return executable.length && ![executable hasPrefix:[bundle stringByAppendingString:@"/"]];
}

static std::string AppSource(NSDictionary *signature)
{
    environment::InstallFacts facts;
    NSString *bundle = NSBundle.mainBundle.bundlePath;
    facts.receipt = Receipt();
    facts.trollStore = access([bundle stringByAppendingPathComponent:@(environment::kTrollStoreMarker)].UTF8String, F_OK) == 0;
    facts.trollStoreLite = access([bundle stringByAppendingPathComponent:@(environment::kTrollStoreLiteMarker)].UTF8String, F_OK) == 0;
    facts.liveContainer = IsLiveContainerApp();
    facts.jitAvailable = [] { return !DetectedJailbreak().empty() || CodeSigningDebugged(); };
    id team = signature[@"entitlements"][@"com.apple.developer.team-identifier"];
    if ([team isKindOfClass:NSString.class]) facts.discordSigned = environment::isDiscordProductionTeam(Plain(team));
    return environment::appSource(facts);
}

static std::string HookFrameworkVersion(const environment::HookFramework &framework, NSString *imagePath)
{
    NSMutableArray<NSString *> *statuses =
        [@[ @"/var/jb/Library/dpkg/status", @"/var/jb/var/lib/dpkg/status", @"/Library/dpkg/status" ] mutableCopy];
    if (NSString *root = ActiveBootstrapRoot())
        [statuses insertObject:[root stringByAppendingPathComponent:@"var/lib/dpkg/status"] atIndex:0];
    for (NSString *status in statuses) {
        NSString *text = [NSString stringWithContentsOfFile:status encoding:NSUTF8StringEncoding error:nil];
        auto version = text ? environment::dpkgPackageVersion(Plain(text), framework.package) : std::nullopt;
        if (version) return *version;
    }
    NSBundle *bundle = [NSBundle bundleWithPath:imagePath.stringByDeletingLastPathComponent];
    return Plain(bundle.infoDictionary[@"CFBundleShortVersionString"] ?: @"");
}

static std::string FrameworkLabel(const char *imagePath, bool exportsLibhookerAPI)
{
    char resolved[PATH_MAX];
    const char *path = realpath(imagePath, resolved) ? resolved : imagePath;
    auto framework = environment::hookFrameworkForImage(path, exportsLibhookerAPI);
    if (!framework) return "";
    return environment::frameworkLabel(framework->name, HookFrameworkVersion(*framework, @(path)));
}

static std::string InjectionFramework(void)
{
    Dl_info provider = {};
    Dl_info libhooker = {};
    void *messageHook = dlsym(RTLD_DEFAULT, "MSHookMessageEx");
    void *libhookerHooks = dlsym(RTLD_DEFAULT, "LHHookFunctions");
    if (messageHook && dladdr(messageHook, &provider) && provider.dli_fname) {
        bool sameImage = libhookerHooks && dladdr(libhookerHooks, &libhooker) && libhooker.dli_fbase == provider.dli_fbase;
        std::string label = FrameworkLabel(provider.dli_fname, sameImage);
        if (!label.empty()) return label;
    }
    for (uint32_t index = 0; index < _dyld_image_count(); index++) {
        const char *name = _dyld_get_image_name(index);
        if (!name || std::strstr(name, "/System/") || std::strstr(name, "/usr/lib/system/")) continue;
        std::string label = FrameworkLabel(name, false);
        if (!label.empty()) return label;
    }
    return "";
}

static NSString *ImageName(void)
{
    Dl_info info = {};
    if (!dladdr((const void *)&ImageName, &info) || !info.dli_fname) return @"";
    return @(info.dli_fname).lastPathComponent;
}

static NSDictionary *StaticReport(void)
{
    NSMutableDictionary *report = [NSMutableDictionary dictionary];
    NSDictionary *signature = SignatureInfo();
    if (auto anatomy = astral::binary::readAnatomy(Plain(NSBundle.mainBundle.executablePath))) {
        report[@"binary"] = @(astral::binary::anatomyLine(*anatomy, Plain(NSBundle.mainBundle.bundlePath)).c_str());
        if (auto signer = astral::binary::signer(anatomy->signature)) report[@"signer"] = @(astral::binary::signerLine(*signer).c_str());
    }
    std::string build = SysctlText("kern.osversion");
    if (report::isBetaBuild(build)) report[@"os_beta"] = @(build.c_str());
    report[@"device_name"] = DeviceName();
    report[@"source"] = @(AppSource(signature).c_str());
    std::string install = InstallDetails(signature);
    if (!install.empty()) report[@"install"] = @(install.c_str());
    report[@"image"] = ImageName();
    report[@"framework"] = @(InjectionFramework().c_str());
    report[@"jailbreak"] = @(DetectedJailbreak().c_str());
    id channel = NSBundle.mainBundle.infoDictionary[@"RELEASE_CHANNEL"];
    if ([channel isKindOfClass:NSString.class]) report[@"discord_channel"] = channel;
    return report;
}

static NSArray<NSString *> *Strings(const std::vector<std::string> &values)
{
    NSMutableArray *strings = [NSMutableArray array];
    for (const std::string &value : values) [strings addObject:@(value.c_str())];
    return strings;
}

static id FoundationJSON(const nlohmann::json &value)
{
    NSData *data = [@(value.dump().c_str()) dataUsingEncoding:NSUTF8StringEncoding];
    return [NSJSONSerialization JSONObjectWithData:data options:NSJSONReadingFragmentsAllowed error:nil];
}

NSDictionary<NSString *, id> *AstralEnvironmentReport(void)
{
    static NSDictionary *cached;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        cached = StaticReport();
    });
    NSMutableDictionary *environment = [cached mutableCopy];
    std::vector<std::string> markers = report::appleMarkers(DeviceFacts());
    if (std::string stack = report::currentNetworkStackMarker(); !stack.empty()) markers.push_back(stack);
    NSString *home = NSSearchPathForDirectoriesInDomains(NSDocumentDirectory, NSUserDomainMask, YES).firstObject;
    if (std::string leftover = report::leftoverModsMarker(Plain(home)); !leftover.empty()) markers.push_back(leftover);
    environment[@"markers"] = Strings(markers);
    report::HttpsCheck https = report::currentHttpsCheck();
    if (https.completed || !https.failure.empty()) environment[@"https"] = FoundationJSON(report::httpsVerdict(https));
    return environment;
}

void AstralStartHttpsCheck(void)
{
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, kHttpsCheckDelay), dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        NSURLSessionConfiguration *configuration = NSURLSessionConfiguration.ephemeralSessionConfiguration;
        configuration.timeoutIntervalForRequest = 15;
        NSURLSession *session = [NSURLSession sessionWithConfiguration:configuration
                                                              delegate:[AstralHttpsChainRecorder new]
                                                         delegateQueue:nil];
        NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:[NSURL URLWithString:kHttpsProbeURL]];
        request.HTTPMethod = @"HEAD";
        [[session dataTaskWithRequest:request
                    completionHandler:^(__unused NSData *data, __unused NSURLResponse *response, NSError *error) {
                        if (error && !report::currentHttpsCheck().completed) {
                            report::HttpsCheck failed;
                            failed.failure = Plain(error.domain) + " " + std::to_string((long)error.code);
                            report::recordHttpsCheck(std::move(failed));
                        }
                        [session finishTasksAndInvalidate];
                    }] resume];
    });
}
