#import "ConfirmSheet.h"

#import <UIKit/UIKit.h>

#import "PhoenixImage.h"

static UIFont *DiscordFont(CGFloat size, UIFontWeight weight)
{
    NSString *name = weight >= UIFontWeightBold ? @"ggsans-Bold" : weight >= UIFontWeightSemibold ? @"ggsans-Semibold" : @"ggsans-Normal";
    return [UIFont fontWithName:name size:size] ?: [UIFont systemFontOfSize:size weight:weight];
}

static const CGFloat kImageSide = 160;
static const CGFloat kSpacing = 16;
static const CGFloat kIconTile = 64;
static const CGFloat kIconGlyph = 34;
static const CGFloat kDetailIcon = 20;

static UIColor *Blurple(void)
{
    return [UIColor colorWithRed:0x58 / 255.0 green:0x65 / 255.0 blue:0xF2 / 255.0 alpha:1];
}

static UIImage *AssetImage(NSString *uri)
{
    NSString *name = [uri hasPrefix:@"res://"] ? [uri substringFromIndex:6] : uri;
    if (!name.length) return nil;
    return [[UIImage imageNamed:name] imageWithRenderingMode:UIImageRenderingModeAlwaysTemplate];
}

@interface AstralConfirmSheet : UIViewController <UIAdaptivePresentationControllerDelegate>
@property (nonatomic, copy) NSDictionary<NSString *, id> *spec;
@property (nonatomic, copy) void (^completion)(BOOL accepted);
@property (nonatomic) UIStackView *stack;
@end

@implementation AstralConfirmSheet

- (void)finish:(BOOL)accepted
{
    void (^completion)(BOOL) = self.completion;
    self.completion = nil;
    if (completion) completion(accepted);
}

- (UILabel *)labelWithText:(NSString *)text font:(UIFont *)font color:(UIColor *)color
{
    UILabel *label = [UILabel new];
    label.text = text;
    label.font = font;
    label.textColor = color;
    label.numberOfLines = 0;
    label.textAlignment = NSTextAlignmentCenter;
    return label;
}

- (UIButton *)buttonWithTitle:(NSString *)title filled:(BOOL)filled action:(SEL)action
{
    UIButtonConfiguration *configuration =
        filled ? UIButtonConfiguration.filledButtonConfiguration : UIButtonConfiguration.grayButtonConfiguration;
    configuration.title = title;
    configuration.cornerStyle = UIButtonConfigurationCornerStyleCapsule;
    if (filled) configuration.baseBackgroundColor = [self.spec[@"destructive"] boolValue] ? UIColor.systemRedColor : Blurple();
    if (filled) configuration.baseForegroundColor = UIColor.whiteColor;
    UIButton *button = [UIButton buttonWithConfiguration:configuration primaryAction:nil];
    [button addTarget:self action:action forControlEvents:UIControlEventTouchUpInside];
    [button.heightAnchor constraintEqualToConstant:48].active = YES;
    return button;
}

- (UIImageView *)imageViewLoading:(NSString *)address
{
    UIImageView *imageView = [UIImageView new];
    imageView.contentMode = UIViewContentModeScaleAspectFit;
    [imageView.widthAnchor constraintEqualToConstant:kImageSide].active = YES;
    [imageView.heightAnchor constraintEqualToConstant:kImageSide].active = YES;
    NSURL *url = [NSURL URLWithString:address];
    if (!url) return imageView;
    __weak UIImageView *weakImageView = imageView;
    [[NSURLSession.sharedSession dataTaskWithURL:url
                               completionHandler:^(NSData *data, __unused NSURLResponse *response, __unused NSError *error) {
                                   UIImage *image = data ? [UIImage imageWithData:data] : nil;
                                   if (!image) return;
                                   dispatch_async(dispatch_get_main_queue(), ^{
                                       weakImageView.image = image;
                                   });
                               }] resume];
    return imageView;
}

- (UIView *)iconTile:(NSString *)uri
{
    UIView *tile = [UIView new];
    tile.backgroundColor = UIColor.tertiarySystemFillColor;
    tile.layer.cornerRadius = 18;
    tile.layer.cornerCurve = kCACornerCurveContinuous;
    [tile.widthAnchor constraintEqualToConstant:kIconTile].active = YES;
    [tile.heightAnchor constraintEqualToConstant:kIconTile].active = YES;
    UIImageView *glyph = [[UIImageView alloc] initWithImage:AssetImage(uri)];
    glyph.tintColor = UIColor.labelColor;
    glyph.contentMode = UIViewContentModeScaleAspectFit;
    glyph.translatesAutoresizingMaskIntoConstraints = NO;
    [tile addSubview:glyph];
    [NSLayoutConstraint activateConstraints:@[
        [glyph.centerXAnchor constraintEqualToAnchor:tile.centerXAnchor],
        [glyph.centerYAnchor constraintEqualToAnchor:tile.centerYAnchor],
        [glyph.widthAnchor constraintEqualToConstant:kIconGlyph],
        [glyph.heightAnchor constraintEqualToConstant:kIconGlyph],
    ]];
    return tile;
}

- (UIView *)detailsCard:(NSArray *)details
{
    UIStackView *rows = [UIStackView new];
    rows.axis = UILayoutConstraintAxisVertical;
    rows.spacing = 12;
    rows.layoutMargins = UIEdgeInsetsMake(14, 16, 14, 16);
    rows.layoutMarginsRelativeArrangement = YES;
    rows.backgroundColor = UIColor.tertiarySystemFillColor;
    rows.layer.cornerRadius = 16;
    rows.layer.cornerCurve = kCACornerCurveContinuous;
    for (NSDictionary *detail in details) {
        if (![detail isKindOfClass:NSDictionary.class] || ![detail[@"text"] isKindOfClass:NSString.class]) continue;
        UIImageView *icon =
            [[UIImageView alloc] initWithImage:AssetImage([detail[@"icon"] isKindOfClass:NSString.class] ? detail[@"icon"] : @"")];
        icon.tintColor = UIColor.secondaryLabelColor;
        icon.contentMode = UIViewContentModeScaleAspectFit;
        [icon.widthAnchor constraintEqualToConstant:kDetailIcon].active = YES;
        [icon.heightAnchor constraintEqualToConstant:kDetailIcon].active = YES;
        UILabel *text = [self labelWithText:detail[@"text"] font:DiscordFont(15, UIFontWeightMedium) color:UIColor.labelColor];
        text.textAlignment = NSTextAlignmentNatural;
        UIStackView *row = [[UIStackView alloc] initWithArrangedSubviews:@[ icon, text ]];
        row.spacing = 12;
        row.alignment = UIStackViewAlignmentCenter;
        [rows addArrangedSubview:row];
    }
    return rows;
}

- (CGFloat)fittingHeight
{
    [self loadViewIfNeeded];
    CGFloat width = UIScreen.mainScreen.bounds.size.width - kSpacing * 2;
    CGSize size = [self.stack systemLayoutSizeFittingSize:CGSizeMake(width, UILayoutFittingCompressedSize.height)
                            withHorizontalFittingPriority:UILayoutPriorityRequired
                                  verticalFittingPriority:UILayoutPriorityFittingSizeLevel];
    return size.height + kSpacing * 3;
}

- (void)viewDidLoad
{
    [super viewDidLoad];
    self.view.backgroundColor = UIColor.systemBackgroundColor;
    UIStackView *stack = [UIStackView new];
    stack.axis = UILayoutConstraintAxisVertical;
    stack.alignment = UIStackViewAlignmentCenter;
    stack.spacing = kSpacing;
    stack.translatesAutoresizingMaskIntoConstraints = NO;
    self.stack = stack;
    if (((NSString *)self.spec[@"image"]).length) [stack addArrangedSubview:[self imageViewLoading:((NSString *)self.spec[@"image"])]];
    if (((NSString *)self.spec[@"icon"]).length) [stack addArrangedSubview:[self iconTile:self.spec[@"icon"]]];
    UILabel *title = [self labelWithText:((NSString *)self.spec[@"title"]) font:DiscordFont(20, UIFontWeightBold) color:UIColor.labelColor];
    [stack addArrangedSubview:title];
    if (((NSString *)self.spec[@"subtitle"]).length) {
        UILabel *subtitle = [self labelWithText:self.spec[@"subtitle"]
                                           font:DiscordFont(13, UIFontWeightMedium)
                                          color:UIColor.secondaryLabelColor];
        [stack addArrangedSubview:subtitle];
        [stack setCustomSpacing:4 afterView:title];
    }
    if (((NSString *)self.spec[@"message"]).length)
        [stack addArrangedSubview:[self labelWithText:((NSString *)self.spec[@"message"])
                                                 font:DiscordFont(15, UIFontWeightRegular)
                                                color:UIColor.secondaryLabelColor]];
    NSArray *details = [self.spec[@"details"] isKindOfClass:NSArray.class] ? self.spec[@"details"] : @[];
    UIView *card = details.count ? [self detailsCard:details] : nil;
    if (card) [stack addArrangedSubview:card];
    UIStackView *buttons = [[UIStackView alloc] initWithArrangedSubviews:@[
        [self buttonWithTitle:((NSString *)self.spec[@"cancel"]) filled:NO action:@selector(cancelPressed)],
        [self buttonWithTitle:((NSString *)self.spec[@"confirm"]) filled:YES action:@selector(confirmPressed)]
    ]];
    buttons.spacing = 12;
    buttons.distribution = UIStackViewDistributionFillEqually;
    [stack addArrangedSubview:buttons];
    [self.view addSubview:stack];
    UILayoutGuide *safe = self.view.safeAreaLayoutGuide;
    [NSLayoutConstraint activateConstraints:@[
        [stack.topAnchor constraintEqualToAnchor:safe.topAnchor constant:kSpacing * 2],
        [stack.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor constant:kSpacing],
        [stack.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor constant:-kSpacing],
        [buttons.widthAnchor constraintEqualToAnchor:stack.widthAnchor],
    ]];
    if (card) [card.widthAnchor constraintEqualToAnchor:stack.widthAnchor].active = YES;
}

- (void)confirmPressed
{
    [self dismissViewControllerAnimated:YES completion:nil];
    [self finish:YES];
}

- (void)cancelPressed
{
    [self dismissViewControllerAnimated:YES completion:nil];
    [self finish:NO];
}

- (void)presentationControllerDidDismiss:(__unused UIPresentationController *)presentationController
{
    [self finish:NO];
}

@end

BOOL AstralShowConfirm(NSDictionary<NSString *, id> *spec, void (^completion)(BOOL accepted))
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIViewController *top = AstralPhoenixWindow().rootViewController;
        while (top.presentedViewController) top = top.presentedViewController;
        if (!top) {
            completion(NO);
            return;
        }
        AstralConfirmSheet *sheet = [AstralConfirmSheet new];
        sheet.spec = spec;
        sheet.completion = completion;
        sheet.modalPresentationStyle = UIModalPresentationPageSheet;
        CGFloat height = [sheet fittingHeight];
        sheet.sheetPresentationController.detents = @[ [UISheetPresentationControllerDetent
            customDetentWithIdentifier:nil
                              resolver:^CGFloat(id<UISheetPresentationControllerDetentResolutionContext> context) {
                                  return MIN(height, context.maximumDetentValue);
                              }] ];
        sheet.sheetPresentationController.prefersGrabberVisible = YES;
        sheet.presentationController.delegate = sheet;
        [top presentViewController:sheet animated:YES completion:nil];
    });
    return YES;
}
