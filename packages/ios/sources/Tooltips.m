#import "Tooltips.h"

#import <objc/runtime.h>

#import "PhoenixImage.h"

static NSString *const kTapPrefix = @"astral.tap.";
static const CGFloat kMaxTextWidth = 230;
static const CGFloat kPaddingX = 14;
static const CGFloat kPaddingY = 10;
static const CGFloat kArrowHeight = 8;
static const CGFloat kArrowHalfWidth = 8;
static const CGFloat kAnchorGap = 3;
static const CGFloat kEdgeInset = 8;
static const CGFloat kRadius = 8;
static const NSTimeInterval kVisible = 3.0;
static const NSTimeInterval kKeyboardGuard = 0.6;

static CGPoint gLastTouch;
static __weak UIView *gAnchor;
static UIView *gTooltipView;
static UITouch *gOpeningTouch;
static CFAbsoluteTime gKeepKeyboardUntil;
static BOOL (*gTextViewResign)(id, SEL);
static BOOL (*gTextFieldResign)(id, SEL);
static BOOL (*gEndEditing)(id, SEL, BOOL);

static UIView *TapAnchor(UIView *view)
{
    for (UIView *current = view; current; current = current.superview) {
        if ([current.accessibilityIdentifier hasPrefix:kTapPrefix]) return current;
    }
    return nil;
}

static void DismissTooltip(void)
{
    UIView *shown = gTooltipView;
    gTooltipView = nil;
    if (!shown) return;
    [UIView animateWithDuration:0.15
        animations:^{
            shown.alpha = 0;
        }
        completion:^(__unused BOOL finished) {
            [shown removeFromSuperview];
        }];
}

void AstralNoteTouch(UITouch *touch)
{
    if (touch.phase == UITouchPhaseBegan) {
        if (gTooltipView && touch != gOpeningTouch) DismissTooltip();
        gLastTouch = [touch locationInView:nil];
        gAnchor = TapAnchor(touch.view);
        gOpeningTouch = touch;
    }
    if (gAnchor && TapAnchor(touch.view) == gAnchor) {
        gKeepKeyboardUntil = CFAbsoluteTimeGetCurrent() + kKeyboardGuard;
    }
}

static BOOL KeepingKeyboard(void)
{
    return CFAbsoluteTimeGetCurrent() < gKeepKeyboardUntil;
}

static BOOL TextViewResign(id self, SEL selector)
{
    if (KeepingKeyboard()) return NO;
    return gTextViewResign(self, selector);
}

static BOOL TextFieldResign(id self, SEL selector)
{
    if (KeepingKeyboard()) return NO;
    return gTextFieldResign(self, selector);
}

static BOOL EndEditing(id self, SEL selector, BOOL force)
{
    if (KeepingKeyboard()) return NO;
    return gEndEditing(self, selector, force);
}

void AstralInstallTooltips(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        Method textView = class_getInstanceMethod(UITextView.class, @selector(resignFirstResponder));
        gTextViewResign = (BOOL (*)(id, SEL))method_setImplementation(textView, (IMP)TextViewResign);
        Method textField = class_getInstanceMethod(UITextField.class, @selector(resignFirstResponder));
        gTextFieldResign = (BOOL (*)(id, SEL))method_setImplementation(textField, (IMP)TextFieldResign);
        Method endEditing = class_getInstanceMethod(UIView.class, @selector(endEditing:));
        gEndEditing = (BOOL (*)(id, SEL, BOOL))method_setImplementation(endEditing, (IMP)EndEditing);
    });
}

static UIColor *BubbleColor(UIView *window)
{
    BOOL dark = window.traitCollection.userInterfaceStyle != UIUserInterfaceStyleLight;
    return dark ? [UIColor colorWithRed:0x3A / 255.0 green:0x3B / 255.0 blue:0x41 / 255.0 alpha:1]
                : [UIColor colorWithRed:0xFF / 255.0 green:0xFF / 255.0 blue:0xFF / 255.0 alpha:1];
}

static UIColor *TextColor(UIView *window)
{
    BOOL dark = window.traitCollection.userInterfaceStyle != UIUserInterfaceStyleLight;
    return dark ? [UIColor colorWithRed:0xDB / 255.0 green:0xDE / 255.0 blue:0xE1 / 255.0 alpha:1]
                : [UIColor colorWithRed:0x31 / 255.0 green:0x33 / 255.0 blue:0x38 / 255.0 alpha:1];
}

static UIBezierPath *BubblePath(CGSize body, CGFloat arrowX, BOOL arrowUp)
{
    CGFloat top = arrowUp ? kArrowHeight : 0;
    UIBezierPath *path = [UIBezierPath bezierPathWithRoundedRect:CGRectMake(0, top, body.width, body.height) cornerRadius:kRadius];
    UIBezierPath *arrow = [UIBezierPath bezierPath];
    if (arrowUp) {
        [arrow moveToPoint:CGPointMake(arrowX - kArrowHalfWidth, kArrowHeight + 0.5)];
        [arrow addLineToPoint:CGPointMake(arrowX, 0)];
        [arrow addLineToPoint:CGPointMake(arrowX + kArrowHalfWidth, kArrowHeight + 0.5)];
    } else {
        [arrow moveToPoint:CGPointMake(arrowX - kArrowHalfWidth, body.height - 0.5)];
        [arrow addLineToPoint:CGPointMake(arrowX, body.height + kArrowHeight)];
        [arrow addLineToPoint:CGPointMake(arrowX + kArrowHalfWidth, body.height - 0.5)];
    }
    [arrow closePath];
    [path appendPath:arrow];
    return path;
}

static CGRect AnchorFrame(UIWindow *window)
{
    UIView *anchor = gAnchor;
    if (anchor.window == window) return [anchor convertRect:anchor.bounds toView:window];
    return CGRectMake(gLastTouch.x, gLastTouch.y, 0, 0);
}

static UIView *TooltipView(NSString *text, UIWindow *window)
{
    UILabel *label = [UILabel new];
    label.text = text;
    label.textColor = TextColor(window);
    label.font = [UIFont systemFontOfSize:13 weight:UIFontWeightMedium];
    label.numberOfLines = 0;
    CGSize fitted = [label sizeThatFits:CGSizeMake(kMaxTextWidth, CGFLOAT_MAX)];
    CGSize body = CGSizeMake(ceil(fitted.width) + kPaddingX * 2, ceil(fitted.height) + kPaddingY * 2);
    CGRect anchor = AnchorFrame(window);
    UIEdgeInsets safe = window.safeAreaInsets;
    CGFloat height = body.height + kArrowHeight;
    CGFloat left = kEdgeInset;
    CGFloat right = window.bounds.size.width - body.width - kEdgeInset;
    CGFloat x = MAX(left, MIN(CGRectGetMidX(anchor) - body.width / 2, right));
    CGFloat belowY = CGRectGetMaxY(anchor) + kAnchorGap;
    BOOL below = belowY + height <= window.bounds.size.height - safe.bottom - kEdgeInset;
    CGFloat y = below ? belowY : MAX(safe.top + kEdgeInset, CGRectGetMinY(anchor) - height - kAnchorGap);
    CGFloat arrowX = MAX(kRadius + kArrowHalfWidth, MIN(CGRectGetMidX(anchor) - x, body.width - kRadius - kArrowHalfWidth));
    UIView *bubble = [[UIView alloc] initWithFrame:CGRectMake(x, y, body.width, height)];
    bubble.userInteractionEnabled = NO;
    bubble.isAccessibilityElement = YES;
    bubble.accessibilityLabel = text;
    CAShapeLayer *shape = [CAShapeLayer layer];
    shape.path = BubblePath(body, arrowX, below).CGPath;
    shape.fillColor = BubbleColor(window).CGColor;
    shape.shadowColor = UIColor.blackColor.CGColor;
    shape.shadowOpacity = 0.24;
    shape.shadowRadius = 6;
    shape.shadowOffset = CGSizeMake(0, 2);
    [bubble.layer addSublayer:shape];
    label.frame = CGRectMake(kPaddingX, (below ? kArrowHeight : 0) + kPaddingY, ceil(fitted.width), ceil(fitted.height));
    [bubble addSubview:label];
    return bubble;
}

void AstralShowTooltip(NSString *text)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIWindow *window = AstralPhoenixWindow();
        if (!window) return;
        [gTooltipView removeFromSuperview];
        UIView *bubble = TooltipView(text, window);
        bubble.alpha = 0;
        [window addSubview:bubble];
        gTooltipView = bubble;
        [UIView animateWithDuration:0.15
                         animations:^{
                             bubble.alpha = 1;
                         }];
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(kVisible * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
            if (gTooltipView == bubble) DismissTooltip();
        });
    });
}
