#import "MessageMenu.h"

#import <UIKit/UIKit.h>
#import <objc/runtime.h>

#include "AstralLuaHost.h"
#include "PhoenixWire.h"

static void (*gOriginalSendEvent)(UIWindow *, SEL, UIEvent *);
static NSArray<NSString *> *gMessagePrefixes;

static uint64_t MessageIn(UIView *view, int depth)
{
    NSString *identifier = view.accessibilityIdentifier;
    for (NSString *prefix in gMessagePrefixes) {
        if (![identifier hasPrefix:prefix]) continue;
        unsigned long long message = strtoull([identifier substringFromIndex:prefix.length].UTF8String, NULL, 10);
        if (message) return message;
    }
    if (depth > 12) return 0;
    for (UIView *child in view.subviews) {
        uint64_t message = MessageIn(child, depth + 1);
        if (message) return message;
    }
    return 0;
}

static uint64_t MessageAt(UIView *view)
{
    for (UIView *current = view; current; current = current.superview) {
        if ([current.accessibilityIdentifier isEqualToString:@"message-row"]) return MessageIn(current, 0);
        if ([current.accessibilityIdentifier hasPrefix:@"message.content:"]) return MessageIn(current, 0);
    }
    return 0;
}

static void SendEvent(UIWindow *self, SEL selector, UIEvent *event)
{
    if (event.type == UIEventTypeTouches) {
        for (UITouch *touch in event.allTouches) {
            if (touch.phase != UITouchPhaseBegan || !touch.view) continue;
            uint64_t message = MessageAt(touch.view);
            if (message) astral::luahost::setMenuMessage(astral_wire_current_channel(), message);
            break;
        }
    }
    gOriginalSendEvent(self, selector, event);
}

static UIViewController *TopController(void)
{
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class]) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows) {
            if (!window.isKeyWindow) continue;
            UIViewController *controller = window.rootViewController;
            while (controller.presentedViewController) controller = controller.presentedViewController;
            return controller;
        }
    }
    return nil;
}

void AstralDismissMessageMenu(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIViewController *top = TopController();
        if (top.presentingViewController) [top dismissViewControllerAnimated:YES completion:nil];
    });
}

void AstralInstallMessageMenu(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gMessagePrefixes = @[ @"message.content:", @"message.author:", @"message.avatar:", @"message.timestamp:" ];
        Method method = class_getInstanceMethod(UIWindow.class, @selector(sendEvent:));
        gOriginalSendEvent = (void (*)(UIWindow *, SEL, UIEvent *))method_setImplementation(method, (IMP)SendEvent);
        astral_wire_set_message_menu_provider(AstralLuaMessageMenu);
    });
}
