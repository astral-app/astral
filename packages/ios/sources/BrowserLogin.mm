#import "BrowserLogin.h"

#import <AuthenticationServices/AuthenticationServices.h>
#import <SafariServices/SafariServices.h>
#import <UIKit/UIKit.h>
#import <objc/runtime.h>

static void (*gOriginalPerformRequests)(id, SEL);
static void (*gOriginalPerformRequestsWithOptions)(id, SEL, ASAuthorizationControllerRequestOptions);

struct PhoenixPasskeyCapabilities {
    BOOL nativeModule;
    BOOL authorizationDelegate;
    BOOL failureDelegate;
};

static PhoenixPasskeyCapabilities InspectPhoenixPasskeys(void)
{
    Class module = NSClassFromString(@"LibdiscordHost.PasskeysModule");
    SEL completed = NSSelectorFromString(@"authorizationController:didCompleteWithAuthorization:");
    SEL failed = NSSelectorFromString(@"authorizationController:didCompleteWithError:");
    return {module != Nil, module && class_getInstanceMethod(module, completed), module && class_getInstanceMethod(module, failed)};
}

static BOOL HasDiscordPasskeyAssertion(ASAuthorizationController *controller)
{
    for (ASAuthorizationRequest *request in controller.authorizationRequests) {
        if (![request conformsToProtocol:@protocol(ASAuthorizationPublicKeyCredentialAssertionRequest)]) continue;
        id<ASAuthorizationPublicKeyCredentialAssertionRequest> assertion = (id)request;
        NSString *relyingParty = assertion.relyingPartyIdentifier.lowercaseString;
        if ([relyingParty isEqualToString:@"discord.com"] || [relyingParty hasSuffix:@".discord.com"]) return YES;
    }
    return NO;
}

static BOOL PhoenixHandlesPasskeys(void)
{
    PhoenixPasskeyCapabilities capabilities = InspectPhoenixPasskeys();
    return capabilities.authorizationDelegate && capabilities.failureDelegate;
}

static NSString *SafariExtensionIdentifier(void)
{
    NSString *path = [NSBundle.mainBundle.bundlePath stringByAppendingPathComponent:@"PlugIns/OpenInAstral.appex"];
    NSBundle *extension = [NSBundle bundleWithPath:path];
    return [extension objectForInfoDictionaryKey:@"CFBundleIdentifier"];
}

static void PerformOriginal(ASAuthorizationController *controller, SEL selector, ASAuthorizationControllerRequestOptions options,
                            BOOL hasOptions)
{
    if (hasOptions) {
        if (gOriginalPerformRequestsWithOptions) gOriginalPerformRequestsWithOptions(controller, selector, options);
    } else if (gOriginalPerformRequests) {
        gOriginalPerformRequests(controller, selector);
    }
}

static void OpenSafariLogin(ASAuthorizationController *controller, SEL selector, ASAuthorizationControllerRequestOptions options,
                            BOOL hasOptions)
{
    NSString *state = NSUUID.UUID.UUIDString.lowercaseString;
    NSString *url = [NSString stringWithFormat:@"https://discord.com/login#astral-browser-login=%@", state];
    [UIApplication.sharedApplication openURL:[NSURL URLWithString:url]
        options:@{}
        completionHandler:^(BOOL success) {
            if (!success) PerformOriginal(controller, selector, options, hasOptions);
        }];
}

static void BeginBrowserLogin(ASAuthorizationController *controller, SEL selector, ASAuthorizationControllerRequestOptions options,
                              BOOL hasOptions)
{
    if (PhoenixHandlesPasskeys()) {
        PerformOriginal(controller, selector, options, hasOptions);
        return;
    }
    if (!HasDiscordPasskeyAssertion(controller)) {
        PerformOriginal(controller, selector, options, hasOptions);
        return;
    }

    NSString *identifier = SafariExtensionIdentifier();
    if (identifier.length == 0) {
        PerformOriginal(controller, selector, options, hasOptions);
        return;
    }

    if (@available(iOS 26.2, *)) {
        [SFSafariExtensionManager getStateOfExtensionWithIdentifier:identifier
                                                  completionHandler:^(SFSafariExtensionState *state, NSError *error) {
                                                      dispatch_async(dispatch_get_main_queue(), ^{
                                                          if (error || !state.isEnabled) {
                                                              PerformOriginal(controller, selector, options, hasOptions);
                                                              return;
                                                          }
                                                          OpenSafariLogin(controller, selector, options, hasOptions);
                                                      });
                                                  }];
        return;
    }

    OpenSafariLogin(controller, selector, options, hasOptions);
}

static void PerformRequests(ASAuthorizationController *controller, SEL selector)
{
    BeginBrowserLogin(controller, selector, 0, NO);
}

static void PerformRequestsWithOptions(ASAuthorizationController *controller, SEL selector, ASAuthorizationControllerRequestOptions options)
{
    BeginBrowserLogin(controller, selector, options, YES);
}

void AstralInstallBrowserLogin(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        Class controller = NSClassFromString(@"ASAuthorizationController");
        Method perform = controller ? class_getInstanceMethod(controller, @selector(performRequests)) : nullptr;
        if (perform) gOriginalPerformRequests = (void (*)(id, SEL))method_setImplementation(perform, (IMP)PerformRequests);
        SEL optionsSelector = NSSelectorFromString(@"performRequestsWithOptions:");
        Method performWithOptions = controller ? class_getInstanceMethod(controller, optionsSelector) : nullptr;
        if (performWithOptions)
            gOriginalPerformRequestsWithOptions = (void (*)(id, SEL, ASAuthorizationControllerRequestOptions))method_setImplementation(
                performWithOptions, (IMP)PerformRequestsWithOptions);
    });
}

NSDictionary<NSString *, NSNumber *> *AstralBrowserLoginStatus(void)
{
    PhoenixPasskeyCapabilities capabilities = InspectPhoenixPasskeys();
    return @{
        @"nativeModule" : @(capabilities.nativeModule),
        @"authorizationDelegate" : @(capabilities.authorizationDelegate),
        @"failureDelegate" : @(capabilities.failureDelegate),
        @"nativePasskeys" : @((BOOL)(capabilities.authorizationDelegate && capabilities.failureDelegate))
    };
}
