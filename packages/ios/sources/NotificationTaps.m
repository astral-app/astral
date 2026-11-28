#import "NotificationTaps.h"

#import <UIKit/UIKit.h>
#import <UserNotifications/UserNotifications.h>
#import <objc/runtime.h>

#import "AstralCore.h"

NSString *const AstralNotificationLinkKey = @"astral_link";
NSString *const AstralNotificationReplyKey = @"astral_reply";
static NSString *const kReplyAction = @"astral.reply";

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

static NSString *Field(NSDictionary *reply, NSString *key)
{
    id value = reply[key];
    return [value isKindOfClass:NSString.class] ? value : @"";
}

static BOOL SendReply(UNNotificationResponse *response)
{
    if (![response.actionIdentifier isEqualToString:kReplyAction]) return NO;
    if (![response isKindOfClass:UNTextInputNotificationResponse.class]) return NO;
    NSDictionary *reply = response.notification.request.content.userInfo[AstralNotificationReplyKey];
    if (![reply isKindOfClass:NSDictionary.class]) return NO;
    NSString *text = ((UNTextInputNotificationResponse *)response).userText ?: @"";
    astral_ios_notification_reply(AstralTextOf(Field(reply, @"plugin")), AstralTextOf(Field(reply, @"channel_id")),
                                  AstralTextOf(Field(reply, @"message_id")), AstralTextOf(text));
    return YES;
}

static void DidReceiveResponse(id self, SEL selector, UNUserNotificationCenter *center, UNNotificationResponse *response,
                               void (^completion)(void))
{
    if (SendReply(response)) {
        completion();
        return;
    }
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

void AstralWithReplyCategory(NSDictionary *reply, void (^ready)(NSString *category))
{
    NSString *title = Field(reply, @"title");
    NSString *send = Field(reply, @"send");
    NSString *placeholder = Field(reply, @"placeholder");
    NSString *category = [NSString stringWithFormat:@"astral.reply.%lu", (unsigned long)@[ title, send, placeholder ].hash];
    UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
    [center getNotificationCategoriesWithCompletionHandler:^(NSSet<UNNotificationCategory *> *existing) {
        for (UNNotificationCategory *known in existing) {
            if ([known.identifier isEqualToString:category]) {
                ready(category);
                return;
            }
        }
        UNTextInputNotificationAction *action = [UNTextInputNotificationAction actionWithIdentifier:kReplyAction
                                                                                              title:title
                                                                                            options:UNNotificationActionOptionNone
                                                                               textInputButtonTitle:send.length ? send : title
                                                                               textInputPlaceholder:placeholder];
        UNNotificationCategory *added = [UNNotificationCategory categoryWithIdentifier:category
                                                                               actions:@[ action ]
                                                                     intentIdentifiers:@[]
                                                                               options:UNNotificationCategoryOptionNone];
        [center setNotificationCategories:[existing setByAddingObject:added]];
        ready(category);
    }];
}
