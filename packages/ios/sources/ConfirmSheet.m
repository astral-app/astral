#import "ConfirmSheet.h"

#import <UIKit/UIKit.h>

#import "PhoenixImage.h"

static const CGFloat kImageSide = 160;
static const CGFloat kSpacing = 16;

@interface AstralConfirmSheet : UIViewController <UIAdaptivePresentationControllerDelegate>
@property (nonatomic, copy) NSDictionary<NSString *, id> *spec;
@property (nonatomic, copy) void (^completion)(BOOL accepted);
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
    if (filled && [self.spec[@"destructive"] boolValue]) configuration.baseBackgroundColor = UIColor.systemRedColor;
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

- (void)viewDidLoad
{
    [super viewDidLoad];
    self.view.backgroundColor = UIColor.systemBackgroundColor;
    UIStackView *stack = [UIStackView new];
    stack.axis = UILayoutConstraintAxisVertical;
    stack.alignment = UIStackViewAlignmentCenter;
    stack.spacing = kSpacing;
    stack.translatesAutoresizingMaskIntoConstraints = NO;
    if (((NSString *)self.spec[@"image"]).length) [stack addArrangedSubview:[self imageViewLoading:((NSString *)self.spec[@"image"])]];
    [stack addArrangedSubview:[self labelWithText:((NSString *)self.spec[@"title"])
                                             font:[UIFont systemFontOfSize:20 weight:UIFontWeightBold]
                                            color:UIColor.labelColor]];
    if (((NSString *)self.spec[@"message"]).length)
        [stack addArrangedSubview:[self labelWithText:((NSString *)self.spec[@"message"])
                                                 font:[UIFont systemFontOfSize:15]
                                                color:UIColor.secondaryLabelColor]];
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
        sheet.sheetPresentationController.detents = @[ UISheetPresentationControllerDetent.mediumDetent ];
        sheet.sheetPresentationController.prefersGrabberVisible = YES;
        sheet.presentationController.delegate = sheet;
        [top presentViewController:sheet animated:YES completion:nil];
    });
    return YES;
}
