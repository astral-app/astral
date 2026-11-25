#import "NotificationTaps.h"

#import <UIKit/UIKit.h>
#import <UserNotifications/UserNotifications.h>
#import <objc/runtime.h>

NSString *const AstralNotificationLinkKey = @"astral_link";

typedef void (*ResponseHandler)(id, SEL, UNUserNotificationCenter *, UNNotificationResponse *, void (^)(void));

static ResponseHandler gOriginalResponseHandler;

static BOOL OpenAstralLink(UNNotificationResponse *response)
{
    NSString *link = response.notification.request.content.userInfo[AstralNotificationLinkKey];
    NSURL *url = [link isKindOfClass:NSString.class] ? [NSURL URLWithString:link] : nil;
    if (!url) return NO;
    dispatch_async(dispatch_get_main_queue(), ^{
        [UIApplication.sharedApplication openURL:url options:@{} completionHandler:nil];
    });
    return YES;
}

static void DidReceiveResponse(id self, SEL selector, UNUserNotificationCenter *center, UNNotificationResponse *response,
                               void (^completion)(void))
{
    if (OpenAstralLink(response)) {
        completion();
        return;
    }
    if (gOriginalResponseHandler) {
        gOriginalResponseHandler(self, selector, center, response, completion);
        return;
    }
    completion();
}

void AstralRouteNotificationTaps(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        static Class routed;
        id<UNUserNotificationCenterDelegate> delegate = UNUserNotificationCenter.currentNotificationCenter.delegate;
        if (!delegate || routed == [delegate class]) return;
        routed = [delegate class];
        SEL selector = @selector(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:);
        Method method = class_getInstanceMethod(routed, selector);
        if (method) {
            gOriginalResponseHandler = (ResponseHandler)method_setImplementation(method, (IMP)DidReceiveResponse);
        } else {
            class_addMethod(routed, selector, (IMP)DidReceiveResponse, "v@:@@@?");
        }
    });
}
