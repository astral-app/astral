#import "BrowserAuth.h"

#import <UIKit/UIKit.h>
#import <WebKit/WebKit.h>

#import "AstralCore.h"
#import "PhoenixImage.h"

@interface AstralWebAuthController : UIViewController <WKNavigationDelegate, UIAdaptivePresentationControllerDelegate>
@property (nonatomic) uint64_t request;
@property (nonatomic, strong) WKWebView *webView;
@property (nonatomic, copy) void (^completion)(uint64_t, NSString *, NSString *, NSString *);
@property (nonatomic) BOOL completed;
@end

static NSMutableDictionary<NSNumber *, AstralWebAuthController *> *gWebAuths;

static void PrepareWebAuthStorage(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gWebAuths = [NSMutableDictionary dictionary];
    });
}

static UIViewController *WebAuthTopController(void)
{
    UIViewController *controller = AstralPhoenixWindow().rootViewController;
    while (controller.presentedViewController) controller = controller.presentedViewController;
    return controller;
}

static NSData *CookiesJSON(NSArray<NSHTTPCookie *> *cookies)
{
    NSMutableArray<NSDictionary *> *plain = [NSMutableArray arrayWithCapacity:cookies.count];
    for (NSHTTPCookie *cookie in cookies) {
        [plain addObject:@{
            @"name" : cookie.name ?: @"",
            @"value" : cookie.value ?: @"",
            @"domain" : cookie.domain ?: @"",
            @"path" : cookie.path ?: @"",
        }];
    }
    return [NSJSONSerialization dataWithJSONObject:plain options:0 error:nil] ?: [NSData data];
}

@implementation AstralWebAuthController

- (void)viewDidLoad
{
    [super viewDidLoad];
    self.view.backgroundColor = UIColor.systemBackgroundColor;
    self.title = @"Authorize";
    self.navigationItem.leftBarButtonItem = [[UIBarButtonItem alloc] initWithBarButtonSystemItem:UIBarButtonSystemItemCancel
                                                                                          target:self
                                                                                          action:@selector(cancel)];
    WKWebViewConfiguration *configuration = [WKWebViewConfiguration new];
    configuration.websiteDataStore = WKWebsiteDataStore.defaultDataStore;
    self.webView = [[WKWebView alloc] initWithFrame:CGRectZero configuration:configuration];
    self.webView.navigationDelegate = self;
    self.webView.translatesAutoresizingMaskIntoConstraints = NO;
    [self.view addSubview:self.webView];
    [NSLayoutConstraint activateConstraints:@[
        [self.webView.topAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.topAnchor],
        [self.webView.bottomAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.bottomAnchor],
        [self.webView.leadingAnchor constraintEqualToAnchor:self.view.leadingAnchor],
        [self.webView.trailingAnchor constraintEqualToAnchor:self.view.trailingAnchor],
    ]];
}

- (void)viewDidAppear:(BOOL)animated
{
    [super viewDidAppear:animated];
    self.navigationController.presentationController.delegate = self;
}

- (void)cancel
{
    [self finishWithURL:nil cookies:nil error:@"cancelled" dismiss:YES];
}

- (void)finishWithURL:(NSString *)url cookies:(NSString *)cookies error:(NSString *)error dismiss:(BOOL)dismissController
{
    if (self.completed) return;
    self.completed = YES;
    [self.webView stopLoading];
    uint64_t request = self.request;
    void (^completion)(uint64_t, NSString *, NSString *, NSString *) = self.completion;
    self.completion = nil;
    NSNumber *key = @(request);
    [gWebAuths removeObjectForKey:key];
    UINavigationController *navigation = self.navigationController;
    void (^deliver)(void) = ^{
        if (completion) completion(request, url, cookies, error);
    };
    if (dismissController && navigation.presentingViewController) {
        [navigation dismissViewControllerAnimated:YES completion:deliver];
    } else {
        deliver();
    }
}

- (void)completeCallback:(NSURL *)url
{
    WKHTTPCookieStore *store = self.webView.configuration.websiteDataStore.httpCookieStore;
    [store getAllCookies:^(NSArray<NSHTTPCookie *> *cookies) {
        NSString *values = AstralTakeString(
            astral_ios_web_auth_new_cookies(self.request, AstralTextOfData(CookiesJSON(cookies)), AstralTextOf(url.absoluteString)));
        dispatch_async(dispatch_get_main_queue(), ^{
            [self finishWithURL:url.absoluteString cookies:values ?: @"" error:nil dismiss:YES];
        });
    }];
}

- (void)presentationControllerDidDismiss:(UIPresentationController *)presentationController
{
    [self finishWithURL:nil cookies:nil error:@"cancelled" dismiss:NO];
}

- (void)webView:(WKWebView *)webView
    decidePolicyForNavigationAction:(WKNavigationAction *)navigationAction
                    decisionHandler:(void (^)(WKNavigationActionPolicy))decisionHandler
{
    NSURL *url = navigationAction.request.URL;
    if (navigationAction.targetFrame.isMainFrame && url &&
        astral_ios_web_auth_is_callback(self.request, AstralTextOf(url.absoluteString))) {
        decisionHandler(WKNavigationActionPolicyCancel);
        [self completeCallback:url];
        return;
    }
    decisionHandler(WKNavigationActionPolicyAllow);
}

@end

BOOL AstralOpenPluginWebAuth(uint64_t request, NSString *url, NSString *callbackURL,
                             void (^completion)(uint64_t, NSString *, NSString *, NSString *))
{
    PrepareWebAuthStorage();
    dispatch_async(dispatch_get_main_queue(), ^{
        UIViewController *top = WebAuthTopController();
        if (!top) {
            completion(request, nil, nil, @"unavailable");
            return;
        }
        AstralWebAuthController *controller = [AstralWebAuthController new];
        controller.request = request;
        controller.completion = completion;
        [controller loadViewIfNeeded];
        UINavigationController *navigation = [[UINavigationController alloc] initWithRootViewController:controller];
        navigation.modalPresentationStyle = UIModalPresentationPageSheet;
        gWebAuths[@(request)] = controller;
        WKHTTPCookieStore *store = controller.webView.configuration.websiteDataStore.httpCookieStore;
        [store getAllCookies:^(NSArray<NSHTTPCookie *> *cookies) {
            astral_ios_web_auth_note_initial_cookies(request, AstralTextOfData(CookiesJSON(cookies)), AstralTextOf(callbackURL));
            dispatch_async(dispatch_get_main_queue(), ^{
                if (controller.completed) return;
                UIViewController *presenter = WebAuthTopController();
                if (!presenter) {
                    [controller finishWithURL:nil cookies:nil error:@"unavailable" dismiss:NO];
                    return;
                }
                [presenter presentViewController:navigation
                                        animated:YES
                                      completion:^{
                                          navigation.presentationController.delegate = controller;
                                          [controller.webView loadRequest:[NSURLRequest requestWithURL:[NSURL URLWithString:url]]];
                                      }];
            });
        }];
    });
    return YES;
}

void AstralCancelPluginWebAuth(uint64_t request)
{
    PrepareWebAuthStorage();
    dispatch_async(dispatch_get_main_queue(), ^{
        [gWebAuths[@(request)] cancel];
    });
}
