#import "Toasts.h"

#import <UIKit/UIKit.h>

#import "IconPacks.h"
#import "PhoenixImage.h"

static const NSTimeInterval kDefaultDuration = 5.0;
static const NSTimeInterval kAnimation = 0.22;
static const CGFloat kMaxWidth = 500;
static const CGFloat kIcon = 24;
static const CGFloat kProgressHeight = 3;
static const NSUInteger kMaxShown = 3;

static UIColor *Hex(uint32_t argb)
{
    return [UIColor colorWithRed:((argb >> 16) & 0xFF) / 255.0
                           green:((argb >> 8) & 0xFF) / 255.0
                            blue:(argb & 0xFF) / 255.0
                           alpha:((argb >> 24) & 0xFF) / 255.0];
}

static NSString *Text(NSDictionary *spec, NSString *key)
{
    id value = spec[key];
    return [value isKindOfClass:NSString.class] ? value : @"";
}

static UIImage *IconImage(NSString *uri, BOOL tinted)
{
    NSString *name = [uri hasPrefix:@"res://"] ? [uri substringFromIndex:6] : uri;
    if (!name.length) return nil;
    UIImage *image = AstralIconPackImage(name) ?: [UIImage imageNamed:name];
    return tinted ? [image imageWithRenderingMode:UIImageRenderingModeAlwaysTemplate] : image;
}

@interface AstralToastView : UIView
@property (nonatomic) uint64_t toast;
@property (nonatomic, copy) AstralToastEvent event;
@property (nonatomic, copy) NSDictionary<NSString *, id> *spec;
@property (nonatomic) UIStackView *body;
@property (nonatomic) UIView *progress;
@property (nonatomic) NSLayoutConstraint *progressWidth;
@property (nonatomic) uint64_t generation;
@property (nonatomic) BOOL closing;
@end

static NSMutableArray<AstralToastView *> *gToasts;
static UIStackView *gStack;
static uint64_t gAnonymous;

static void Dismiss(AstralToastView *view);

@implementation AstralToastView

- (instancetype)initWithToast:(uint64_t)toast event:(AstralToastEvent)event
{
    self = [super initWithFrame:CGRectZero];
    if (!self) return nil;
    _toast = toast;
    _event = [event copy];
    self.translatesAutoresizingMaskIntoConstraints = NO;
    self.backgroundColor = Hex(0xFF2C2D32);
    self.layer.cornerRadius = 18;
    self.layer.borderWidth = 1;
    self.layer.borderColor = Hex(0x1FFFFFFF).CGColor;
    self.layer.shadowColor = UIColor.blackColor.CGColor;
    self.layer.shadowOpacity = 0.28;
    self.layer.shadowRadius = 12;
    self.layer.shadowOffset = CGSizeMake(0, 4);
    self.isAccessibilityElement = NO;
    UIPanGestureRecognizer *pan = [[UIPanGestureRecognizer alloc] initWithTarget:self action:@selector(panned:)];
    [self addGestureRecognizer:pan];
    return self;
}

- (UILabel *)label:(NSString *)text font:(UIFont *)font color:(UIColor *)color lines:(NSInteger)lines
{
    UILabel *label = [UILabel new];
    label.text = text;
    label.font = font;
    label.textColor = color;
    label.numberOfLines = lines;
    label.adjustsFontForContentSizeCategory = YES;
    return label;
}

- (UIView *)iconView
{
    NSString *uri = Text(self.spec, @"icon");
    BOOL tinted = ![self.spec[@"tinted_icon"] isEqual:@NO];
    UIImage *image = IconImage(uri, tinted);
    if (!image) return nil;
    UIImageView *view = [[UIImageView alloc] initWithImage:image];
    view.translatesAutoresizingMaskIntoConstraints = NO;
    view.contentMode = UIViewContentModeScaleAspectFit;
    view.tintColor = Hex(0xFFDBDEE1);
    [NSLayoutConstraint activateConstraints:@[
        [view.widthAnchor constraintEqualToConstant:kIcon],
        [view.heightAnchor constraintEqualToConstant:kIcon],
    ]];
    return view;
}

- (UIButton *)closeButton
{
    UIButton *button = [UIButton buttonWithType:UIButtonTypeSystem];
    button.translatesAutoresizingMaskIntoConstraints = NO;
    UIImage *image = IconImage(@"ic_mana_x_small", YES) ?: [UIImage systemImageNamed:@"xmark"];
    [button setImage:image forState:UIControlStateNormal];
    button.tintColor = Hex(0xFF949BA4);
    button.accessibilityLabel = @"Close";
    [button addTarget:self action:@selector(closeTapped) forControlEvents:UIControlEventTouchUpInside];
    [NSLayoutConstraint activateConstraints:@[
        [button.widthAnchor constraintEqualToConstant:32],
        [button.heightAnchor constraintEqualToConstant:32],
    ]];
    return button;
}

- (UIButton *)buttonFor:(NSDictionary *)entry index:(NSUInteger)index
{
    NSString *variant = Text(entry, @"variant");
    BOOL primary = !variant.length || [variant isEqualToString:@"primary"];
    BOOL tertiary = [variant isEqualToString:@"tertiary"];
    UIButton *button = [UIButton buttonWithType:UIButtonTypeSystem];
    button.translatesAutoresizingMaskIntoConstraints = NO;
    [button setTitle:Text(entry, @"content") forState:UIControlStateNormal];
    button.titleLabel.font = [UIFont systemFontOfSize:14 weight:UIFontWeightSemibold];
    [button setTitleColor:UIColor.whiteColor forState:UIControlStateNormal];
    button.backgroundColor = primary ? Hex(0xFF5865F2) : (tertiary ? UIColor.clearColor : Hex(0xFF4E5058));
    button.layer.cornerRadius = 16;
    button.tag = (NSInteger)index;
    [button addTarget:self action:@selector(buttonTapped:) forControlEvents:UIControlEventTouchUpInside];
    [button.heightAnchor constraintEqualToConstant:32].active = YES;
    return button;
}

- (UIStackView *)buttonRow
{
    NSArray *entries = [self.spec[@"buttons"] isKindOfClass:NSArray.class] ? self.spec[@"buttons"] : @[];
    NSMutableArray<UIView *> *buttons = [NSMutableArray array];
    [entries enumerateObjectsUsingBlock:^(id entry, NSUInteger index, BOOL *stop) {
        if ([entry isKindOfClass:NSDictionary.class]) [buttons addObject:[self buttonFor:entry index:index]];
    }];
    if (!buttons.count) return nil;
    UIStackView *row = [[UIStackView alloc] initWithArrangedSubviews:buttons];
    row.axis = UILayoutConstraintAxisHorizontal;
    row.spacing = 5;
    row.distribution = UIStackViewDistributionFillEqually;
    row.layoutMargins = UIEdgeInsetsMake(0, 12, 8, 12);
    row.layoutMarginsRelativeArrangement = YES;
    return row;
}

- (void)render
{
    [self.body removeFromSuperview];
    [self.progress removeFromSuperview];
    NSString *title = Text(self.spec, @"title");
    NSString *content = Text(self.spec, @"content");
    UIStackView *texts = [UIStackView new];
    texts.axis = UILayoutConstraintAxisVertical;
    texts.spacing = 2;
    if (title.length) {
        UIFont *font = [UIFont systemFontOfSize:15 weight:UIFontWeightSemibold];
        [texts addArrangedSubview:[self label:title font:font color:Hex(0xFFF2F3F5) lines:2]];
    }
    if (content.length) {
        UIFont *font = [UIFont systemFontOfSize:13];
        [texts addArrangedSubview:[self label:content font:font color:Hex(0xFFB5BAC1) lines:5]];
    }
    UIStackView *row = [UIStackView new];
    row.axis = UILayoutConstraintAxisHorizontal;
    row.alignment = UIStackViewAlignmentCenter;
    row.spacing = 12;
    row.layoutMargins = UIEdgeInsetsMake(8, 12, 8, 6);
    row.layoutMarginsRelativeArrangement = YES;
    UIView *icon = [self iconView];
    if (icon) [row addArrangedSubview:icon];
    [row addArrangedSubview:texts];
    [row addArrangedSubview:[self closeButton]];
    UIStackView *body = [[UIStackView alloc] initWithArrangedSubviews:@[ row ]];
    body.axis = UILayoutConstraintAxisVertical;
    body.translatesAutoresizingMaskIntoConstraints = NO;
    UIStackView *buttons = [self buttonRow];
    if (buttons) [body addArrangedSubview:buttons];
    [self addSubview:body];
    self.body = body;
    [NSLayoutConstraint activateConstraints:@[
        [body.topAnchor constraintEqualToAnchor:self.topAnchor constant:4],
        [body.bottomAnchor constraintEqualToAnchor:self.bottomAnchor constant:-4],
        [body.leadingAnchor constraintEqualToAnchor:self.leadingAnchor],
        [body.trailingAnchor constraintEqualToAnchor:self.trailingAnchor],
    ]];
    self.accessibilityLabel = content.length ? [NSString stringWithFormat:@"%@, %@", title, content] : title;
    [self renderProgress];
    [self schedule];
}

- (NSNumber *)progressValue
{
    id value = self.spec[@"progress"];
    return [value isKindOfClass:NSNumber.class] ? value : nil;
}

- (NSTimeInterval)duration
{
    id value = self.spec[@"duration"];
    return [value isKindOfClass:NSNumber.class] ? [value doubleValue] / 1000.0 : kDefaultDuration;
}

- (void)renderProgress
{
    NSNumber *progress = [self progressValue];
    if (!progress) return;
    UIView *bar = [UIView new];
    bar.translatesAutoresizingMaskIntoConstraints = NO;
    bar.backgroundColor = Hex(0xFF5865F2);
    bar.layer.cornerRadius = kProgressHeight / 2;
    [self addSubview:bar];
    CGFloat fraction = MAX(0, MIN(1, progress.doubleValue));
    self.progressWidth = [bar.widthAnchor constraintEqualToAnchor:self.widthAnchor multiplier:MAX(fraction, 0.001)];
    [NSLayoutConstraint activateConstraints:@[
        [bar.leadingAnchor constraintEqualToAnchor:self.leadingAnchor],
        [bar.bottomAnchor constraintEqualToAnchor:self.bottomAnchor],
        [bar.heightAnchor constraintEqualToConstant:kProgressHeight],
        self.progressWidth,
    ]];
    self.progress = bar;
    self.clipsToBounds = YES;
}

- (void)schedule
{
    uint64_t generation = ++self.generation;
    NSTimeInterval duration = [self duration];
    if ([self progressValue] || duration <= 0) return;
    __weak AstralToastView *weakSelf = self;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(duration * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        AstralToastView *view = weakSelf;
        if (view && view.generation == generation) Dismiss(view);
    });
}

- (void)closeTapped
{
    Dismiss(self);
}

- (void)buttonTapped:(UIButton *)button
{
    if (self.event) self.event(@"button", [NSString stringWithFormat:@"%ld", (long)button.tag + 1], NO);
}

- (void)panned:(UIPanGestureRecognizer *)pan
{
    CGFloat lift = MIN(0, [pan translationInView:self].y);
    if (pan.state == UIGestureRecognizerStateChanged) {
        self.transform = CGAffineTransformMakeTranslation(0, lift);
        return;
    }
    if (pan.state != UIGestureRecognizerStateEnded && pan.state != UIGestureRecognizerStateCancelled) return;
    if (lift < -24 || [pan velocityInView:self].y < -400) {
        Dismiss(self);
        return;
    }
    [UIView animateWithDuration:kAnimation
                     animations:^{
                         self.transform = CGAffineTransformIdentity;
                     }];
}

@end

static UIStackView *Stack(UIWindow *window)
{
    if (gStack.window == window) return gStack;
    [gStack removeFromSuperview];
    UIStackView *stack = [UIStackView new];
    stack.translatesAutoresizingMaskIntoConstraints = NO;
    stack.axis = UILayoutConstraintAxisVertical;
    stack.spacing = 8;
    stack.alignment = UIStackViewAlignmentFill;
    [window addSubview:stack];
    UILayoutGuide *safe = window.safeAreaLayoutGuide;
    NSLayoutConstraint *width = [stack.widthAnchor constraintEqualToAnchor:safe.widthAnchor multiplier:0.9];
    width.priority = UILayoutPriorityDefaultHigh;
    [NSLayoutConstraint activateConstraints:@[
        [stack.topAnchor constraintEqualToAnchor:safe.topAnchor constant:8],
        [stack.centerXAnchor constraintEqualToAnchor:safe.centerXAnchor],
        [stack.widthAnchor constraintLessThanOrEqualToConstant:kMaxWidth],
        width,
    ]];
    gStack = stack;
    return stack;
}

static AstralToastView *Find(uint64_t toast)
{
    for (AstralToastView *view in gToasts) {
        if (view.toast == toast && !view.closing) return view;
    }
    return nil;
}

static void Dismiss(AstralToastView *view)
{
    if (view.closing) return;
    view.closing = YES;
    view.generation++;
    AstralToastEvent event = view.event;
    view.event = nil;
    [UIView animateWithDuration:kAnimation
        animations:^{
            view.alpha = 0;
            view.transform = CGAffineTransformMakeTranslation(0, -24);
        }
        completion:^(BOOL finished) {
            [UIView animateWithDuration:kAnimation
                animations:^{
                    view.hidden = YES;
                }
                completion:^(BOOL done) {
                    [view removeFromSuperview];
                    [gToasts removeObject:view];
                }];
        }];
    if (event) event(@"", @"", YES);
}

static void Present(UIWindow *window, AstralToastView *view)
{
    UIStackView *stack = Stack(window);
    [window bringSubviewToFront:stack];
    while (gToasts.count >= kMaxShown) Dismiss(gToasts.firstObject);
    [gToasts addObject:view];
    view.alpha = 0;
    view.hidden = YES;
    view.transform = CGAffineTransformMakeTranslation(0, -24);
    [stack addArrangedSubview:view];
    [UIView animateWithDuration:kAnimation
                     animations:^{
                         view.hidden = NO;
                         view.alpha = 1;
                         view.transform = CGAffineTransformIdentity;
                     }];
}

void AstralShowToast(uint64_t toast, NSDictionary<NSString *, id> *spec, AstralToastEvent event)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!gToasts) gToasts = [NSMutableArray array];
        AstralToastView *existing = toast ? Find(toast) : nil;
        if (existing) {
            existing.spec = spec;
            [existing render];
            return;
        }
        UIWindow *window = AstralPhoenixWindow();
        if (!window) {
            if (event) event(@"", @"", YES);
            return;
        }
        AstralToastView *view = [[AstralToastView alloc] initWithToast:toast ?: (UINT64_MAX - ++gAnonymous) event:event];
        view.spec = spec;
        [view render];
        Present(window, view);
    });
}

void AstralCloseToast(uint64_t toast)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralToastView *view = Find(toast);
        if (view) Dismiss(view);
    });
}
