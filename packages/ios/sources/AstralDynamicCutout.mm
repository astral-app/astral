#import "AstralDynamicCutout.h"

#import <UIKit/UIKit.h>
#import <objc/message.h>

#include "AstralCutout.h"

static UIView *gOverlay;
static UIWindow *gWindow;
static BOOL gInitialized;

static std::optional<astral::cutout::Shape> DeviceCutout(void)
{
    if (UIDevice.currentDevice.userInterfaceIdiom != UIUserInterfaceIdiomPhone) return std::nullopt;
    SEL exclusionArea = NSSelectorFromString(@"_exclusionArea");
    if (![UIScreen.mainScreen respondsToSelector:exclusionArea]) return std::nullopt;
    id shape = ((id (*)(id, SEL))objc_msgSend)(UIScreen.mainScreen, exclusionArea);
    SEL rectSelector = NSSelectorFromString(@"rect");
    if (![shape respondsToSelector:rectSelector]) return std::nullopt;
    CGRect rect = ((CGRect (*)(id, SEL))objc_msgSend)(shape, rectSelector);
    return astral::cutout::fromSystem(astral::cutout::Frame{rect.origin.x, rect.origin.y, rect.size.width, rect.size.height});
}

static UIWindow *KeyWindow(void)
{
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class]) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows)
            if (window.isKeyWindow) return window;
    }
    return nil;
}

static void CreateOverlay(void)
{
    auto cutout = DeviceCutout();
    UIWindow *window = KeyWindow();
    if (!cutout || !window) return;
    CGRect frame = CGRectMake(cutout->frame.x, cutout->frame.y, cutout->frame.width, cutout->frame.height);
    if (!CGRectContainsRect(window.bounds, frame)) return;
    if (gOverlay && gWindow == window) return;
    [gOverlay removeFromSuperview];
    gWindow = window;
    gOverlay = [[UIView alloc] initWithFrame:frame];
    gOverlay.backgroundColor = UIColor.blackColor;
    gOverlay.alpha = 0;
    gOverlay.hidden = YES;
    gOverlay.userInteractionEnabled = NO;
    UIRectCorner corners =
        cutout->kind == astral::cutout::Kind::Notch ? UIRectCornerBottomLeft | UIRectCornerBottomRight : UIRectCornerAllCorners;
    CGFloat radius = astral::cutout::cornerRadius(*cutout);
    UIBezierPath *path = [UIBezierPath bezierPathWithRoundedRect:gOverlay.bounds
                                               byRoundingCorners:corners
                                                     cornerRadii:CGSizeMake(radius, radius)];
    CAShapeLayer *mask = [CAShapeLayer layer];
    mask.path = path.CGPath;
    gOverlay.layer.mask = mask;
    auto logo = astral::cutout::logoFrame(*cutout);
    UIImageView *image = [[UIImageView alloc] initWithFrame:CGRectMake(logo.x, logo.y, logo.width, logo.height)];
    image.image = [UIImage imageNamed:@"astral_galaxy"];
    image.tintColor = UIColor.whiteColor;
    image.contentMode = UIViewContentModeScaleAspectFit;
    [gOverlay addSubview:image];
    [window addSubview:gOverlay];
    [window bringSubviewToFront:gOverlay];
}

static void ShowOverlay(void)
{
    CreateOverlay();
    if (!gOverlay || !gOverlay.hidden) return;
    gOverlay.hidden = NO;
    [UIView animateWithDuration:0.2
                     animations:^{
                         gOverlay.alpha = 1;
                     }];
}

static void HideOverlay(void)
{
    if (!gOverlay || gOverlay.hidden) return;
    gOverlay.hidden = YES;
    gOverlay.alpha = 0;
}

void AstralInitializeDynamicCutout(void)
{
    if (gInitialized) return;
    gInitialized = YES;
    if (!DeviceCutout()) return;
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    [center addObserverForName:UIApplicationDidBecomeActiveNotification
                        object:nil
                         queue:NSOperationQueue.mainQueue
                    usingBlock:^(__unused NSNotification *note) {
                        dispatch_async(dispatch_get_main_queue(), ^{
                            ShowOverlay();
                        });
                    }];
    [center addObserverForName:UIApplicationWillResignActiveNotification
                        object:nil
                         queue:NSOperationQueue.mainQueue
                    usingBlock:^(__unused NSNotification *note) {
                        dispatch_async(dispatch_get_main_queue(), ^{
                            HideOverlay();
                        });
                    }];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), dispatch_get_main_queue(), ^{
        CreateOverlay();
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC / 2), dispatch_get_main_queue(), ^{
            ShowOverlay();
        });
    });
}
