#import "AstralDynamicCutout.h"
#import "SvgRaster.h"

#import <UIKit/UIKit.h>
#import <objc/message.h>

#import "AstralCore.h"
#import "AstralState.h"

static UIView *gOverlay;
static UIWindow *gWindow;
static BOOL gInitialized;
static id gHingeInteraction;
static UIWindow *gHingeWindow;
static BOOL gUnderDisplayCameraObserved;

typedef NSArray *(*ReservedRegionQuery)(id, SEL, id, NSUInteger);
typedef void (^HingeUpdateHandler)(id, id);

@protocol HingeInteractionInitializing
- (instancetype)initWithUpdateHandler:(HingeUpdateHandler)handler;
@end

static void ShowOverlay(void);

static void RemoveOverlay(void)
{
    [gOverlay removeFromSuperview];
    gOverlay = nil;
    gWindow = nil;
}

static BOOL OverlayAllowed(UIWindow *window, CGRect frame)
{
    SEL querySelector = NSSelectorFromString(@"reservedRegionsOfKind:options:");
    if (![window respondsToSelector:querySelector]) return YES;

    Class kindClass = NSClassFromString(@"UIViewReservedRegionKind");
    SEL kindSelector = NSSelectorFromString(@"occlusionRegionKind");
    if (!kindClass || ![kindClass respondsToSelector:kindSelector]) return NO;

    id kind = ((id (*)(id, SEL))objc_msgSend)(kindClass, kindSelector);
    if (!kind) return NO;

    NSArray *regions = ((ReservedRegionQuery)objc_msgSend)(window, querySelector, kind, 1);
    BOOL hasActiveOcclusion = NO;
    BOOL hasInactiveOcclusion = NO;
    SEL frameSelector = NSSelectorFromString(@"frame");
    SEL activeSelector = NSSelectorFromString(@"isActive");
    for (id region in regions) {
        if (![region respondsToSelector:frameSelector] || ![region respondsToSelector:activeSelector]) {
            hasInactiveOcclusion = YES;
            gUnderDisplayCameraObserved = YES;
            continue;
        }

        CGRect regionFrame = ((CGRect (*)(id, SEL))objc_msgSend)(region, frameSelector);
        if (!CGRectIntersectsRect(frame, regionFrame)) continue;
        BOOL active = ((BOOL (*)(id, SEL))objc_msgSend)(region, activeSelector);
        if (active) {
            hasActiveOcclusion = YES;
        } else {
            hasInactiveOcclusion = YES;
            gUnderDisplayCameraObserved = YES;
        }
    }

    return astral_ios_cutout_overlay_allowed(hasActiveOcclusion, hasInactiveOcclusion, gUnderDisplayCameraObserved) != 0;
}

static void UpdateOverlayForReservedRegions(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (UIApplication.sharedApplication.applicationState == UIApplicationStateActive) {
            ShowOverlay();
        } else {
            RemoveOverlay();
        }
    });
}

static void ObserveHinge(UIWindow *window)
{
    if (!window || gHingeWindow == window) return;
    if (gHingeWindow && gHingeInteraction) [gHingeWindow removeInteraction:gHingeInteraction];
    gHingeWindow = window;
    Class hingeClass = NSClassFromString(@"UIHingeInteraction");
    SEL initializer = NSSelectorFromString(@"initWithUpdateHandler:");
    if (!hingeClass || ![hingeClass instancesRespondToSelector:initializer]) return;

    HingeUpdateHandler handler = ^(__unused id interaction, __unused id update) {
        UpdateOverlayForReservedRegions();
    };
    id<HingeInteractionInitializing> interaction = (id<HingeInteractionInitializing>)[hingeClass alloc];
    gHingeInteraction = [interaction initWithUpdateHandler:handler];
    if (gHingeInteraction) [window addInteraction:gHingeInteraction];
}

static CGRect SystemCutout(void)
{
    if (UIDevice.currentDevice.userInterfaceIdiom != UIUserInterfaceIdiomPhone) return CGRectNull;
    SEL exclusionArea = NSSelectorFromString(@"_exclusionArea");
    if (![UIScreen.mainScreen respondsToSelector:exclusionArea]) return CGRectNull;
    id shape = ((id (*)(id, SEL))objc_msgSend)(UIScreen.mainScreen, exclusionArea);
    SEL rectSelector = NSSelectorFromString(@"rect");
    if (![shape respondsToSelector:rectSelector]) return CGRectNull;
    return ((CGRect (*)(id, SEL))objc_msgSend)(shape, rectSelector);
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
    CGRect frame = SystemCutout();
    UIWindow *window = KeyWindow();
    if (CGRectIsNull(frame) || !window) return;
    ObserveHinge(window);
    if (!OverlayAllowed(window, frame)) {
        RemoveOverlay();
        return;
    }
    AstralCutout cutout = astral_ios_cutout((AstralRect){frame.origin.x, frame.origin.y, frame.size.width, frame.size.height});
    if (cutout.kind == AstralCutoutKind_Missing || !CGRectContainsRect(window.bounds, frame)) return;
    if (gOverlay && gWindow == window && CGRectEqualToRect(gOverlay.frame, frame)) return;
    RemoveOverlay();
    gWindow = window;
    gOverlay = [[UIView alloc] initWithFrame:frame];
    gOverlay.backgroundColor = UIColor.blackColor;
    gOverlay.alpha = 0;
    gOverlay.hidden = YES;
    gOverlay.userInteractionEnabled = NO;
    UIRectCorner corners =
        cutout.kind == AstralCutoutKind_Notch ? UIRectCornerBottomLeft | UIRectCornerBottomRight : UIRectCornerAllCorners;
    CGFloat radius = cutout.corner_radius;
    UIBezierPath *path = [UIBezierPath bezierPathWithRoundedRect:gOverlay.bounds
                                               byRoundingCorners:corners
                                                     cornerRadii:CGSizeMake(radius, radius)];
    CAShapeLayer *mask = [CAShapeLayer layer];
    mask.path = path.CGPath;
    gOverlay.layer.mask = mask;
    AstralRect logo = cutout.logo;
    UIImageView *image = [[UIImageView alloc] initWithFrame:CGRectMake(logo.x, logo.y, logo.width, logo.height)];
    NSString *assets = [AstralResourceDirectory() stringByAppendingPathComponent:@"assets"];
    NSString *svg = [NSString stringWithContentsOfFile:[assets stringByAppendingPathComponent:@"logo.svg"]
                                              encoding:NSUTF8StringEncoding
                                                 error:nil];
    image.image = AstralRenderSVGOriginal(svg, MAX(logo.width, logo.height));
    image.contentMode = UIViewContentModeScaleAspectFit;
    [gOverlay addSubview:image];
    [window addSubview:gOverlay];
    [window bringSubviewToFront:gOverlay];
}

static void ShowOverlay(void)
{
    ObserveHinge(KeyWindow());
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
    if (CGRectIsNull(SystemCutout())) return;
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
