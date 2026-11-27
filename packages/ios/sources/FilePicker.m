#import "FilePicker.h"

#import <UIKit/UIKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <objc/runtime.h>

#import "PhoenixImage.h"

@interface AstralFilePicker : NSObject <UIDocumentPickerDelegate>
@property (nonatomic, copy) void (^completion)(NSData *, NSString *);
@property (nonatomic, copy) void (^pluginCompletion)(uint64_t, NSData *, NSString *, NSString *);
@property (nonatomic) uint64_t request;
@property (nonatomic, weak) UIViewController *controller;
@end

@interface AstralTextEditorController : UIViewController <UITextViewDelegate>
@property (nonatomic, copy) NSString *titleText;
@property (nonatomic, copy) NSString *initialValue;
@property (nonatomic) NSUInteger maximumLength;
@property (nonatomic, strong) UITextView *textView;
@property (nonatomic, copy) void (^completion)(NSString *);
@end

static AstralFilePicker *gPicker;
static UIView *gToastView;
static uint64_t gToastGeneration;
static IMP gOriginalStartAccessing;

static BOOL StartAccessingResource(id object, SEL selector)
{
    ((BOOL (*)(id, SEL))gOriginalStartAccessing)(object, selector);
    return YES;
}

static void InstallSecurityScopedResourceCompatibility(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        Method method = class_getInstanceMethod(NSURL.class, @selector(startAccessingSecurityScopedResource));
        if (method) gOriginalStartAccessing = method_setImplementation(method, (IMP)StartAccessingResource);
    });
}

static UIViewController *TopController(void)
{
    UIViewController *controller = AstralPhoenixWindow().rootViewController;
    while (controller.presentedViewController) controller = controller.presentedViewController;
    return controller;
}

@implementation AstralFilePicker
- (void)documentPicker:(UIDocumentPickerViewController *)controller didPickDocumentsAtURLs:(NSArray<NSURL *> *)urls
{
    NSURL *url = urls.firstObject;
    NSString *error = nil;
    NSData *data = nil;
    if (!url) {
        error = @"file_unavailable";
    }
    BOOL scoped = [url startAccessingSecurityScopedResource];
    NSFileHandle *handle = url ? [NSFileHandle fileHandleForReadingFromURL:url error:nil] : nil;
    data = [handle readDataOfLength:(1u << 20) + 1];
    if (data.length > (1u << 20)) {
        data = nil;
        error = @"file_too_large";
    } else if (!data && !error) {
        error = @"file_unreadable";
    }
    [handle closeFile];
    if (scoped) [url stopAccessingSecurityScopedResource];
    NSString *name = url.lastPathComponent;
    void (^pluginCompletion)(uint64_t, NSData *, NSString *, NSString *) = self.pluginCompletion;
    void (^completion)(NSData *, NSString *) = self.completion;
    uint64_t request = self.request;
    gPicker = nil;
    if (pluginCompletion) pluginCompletion(request, data, name, error);
    if (completion) completion(data, name);
}

- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller
{
    void (^pluginCompletion)(uint64_t, NSData *, NSString *, NSString *) = self.pluginCompletion;
    uint64_t request = self.request;
    gPicker = nil;
    if (pluginCompletion) pluginCompletion(request, nil, nil, @"cancelled");
}
@end

@implementation AstralTextEditorController
- (void)viewDidLoad
{
    [super viewDidLoad];
    self.view.backgroundColor = UIColor.systemBackgroundColor;
    UINavigationBar *bar = [UINavigationBar new];
    UINavigationItem *item = [[UINavigationItem alloc] initWithTitle:self.titleText];
    item.leftBarButtonItem = [[UIBarButtonItem alloc] initWithBarButtonSystemItem:UIBarButtonSystemItemCancel
                                                                           target:self
                                                                           action:@selector(cancel)];
    item.rightBarButtonItem = [[UIBarButtonItem alloc] initWithBarButtonSystemItem:UIBarButtonSystemItemSave
                                                                            target:self
                                                                            action:@selector(save)];
    bar.items = @[ item ];
    self.textView = [UITextView new];
    self.textView.delegate = self;
    self.textView.text = self.initialValue;
    self.textView.font = [UIFont preferredFontForTextStyle:UIFontTextStyleBody];
    self.textView.backgroundColor = UIColor.secondarySystemBackgroundColor;
    self.textView.layer.cornerRadius = 10;
    self.textView.textContainerInset = UIEdgeInsetsMake(12, 10, 12, 10);
    [self.view addSubview:bar];
    [self.view addSubview:self.textView];
    bar.translatesAutoresizingMaskIntoConstraints = NO;
    self.textView.translatesAutoresizingMaskIntoConstraints = NO;
    UILayoutGuide *safe = self.view.safeAreaLayoutGuide;
    [NSLayoutConstraint activateConstraints:@[
        [bar.topAnchor constraintEqualToAnchor:safe.topAnchor],
        [bar.leadingAnchor constraintEqualToAnchor:self.view.leadingAnchor],
        [bar.trailingAnchor constraintEqualToAnchor:self.view.trailingAnchor],
        [self.textView.topAnchor constraintEqualToAnchor:bar.bottomAnchor constant:12],
        [self.textView.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor constant:16],
        [self.textView.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor constant:-16],
        [self.textView.bottomAnchor constraintEqualToAnchor:safe.bottomAnchor constant:-16],
    ]];
    self.preferredContentSize = CGSizeMake(560, 520);
}

- (BOOL)textView:(UITextView *)textView shouldChangeTextInRange:(NSRange)range replacementText:(NSString *)text
{
    NSString *updated = [textView.text stringByReplacingCharactersInRange:range withString:text];
    return [updated lengthOfBytesUsingEncoding:NSUTF8StringEncoding] <= self.maximumLength;
}

- (void)cancel
{
    [self finish:nil];
}

- (void)save
{
    [self finish:self.textView.text ?: @""];
}

- (void)finish:(NSString *)value
{
    void (^completion)(NSString *) = self.completion;
    self.completion = nil;
    if (completion) completion(value);
    [self dismissViewControllerAnimated:YES completion:nil];
}
@end

void AstralPickFile(NSArray<NSString *> *extensions, void (^completion)(NSData *, NSString *))
{
    InstallSecurityScopedResourceCompatibility();
    if (gPicker) {
        completion(nil, nil);
        return;
    }
    NSMutableArray<UTType *> *types = [NSMutableArray array];
    for (NSString *extension in extensions) {
        UTType *type = [UTType typeWithFilenameExtension:extension];
        if (type) [types addObject:type];
    }
    [types addObjectsFromArray:@[ UTTypePlainText, UTTypeData ]];
    gPicker = [AstralFilePicker new];
    gPicker.completion = completion;
    UIDocumentPickerViewController *picker = [[UIDocumentPickerViewController alloc] initForOpeningContentTypes:types];
    picker.delegate = gPicker;
    UIViewController *top = TopController();
    if (!top) {
        gPicker = nil;
        completion(nil, nil);
        return;
    }
    [top presentViewController:picker animated:YES completion:nil];
}

BOOL AstralPickPluginFile(uint64_t request, NSArray<NSString *> *extensions, void (^completion)(uint64_t, NSData *, NSString *, NSString *))
{
    InstallSecurityScopedResourceCompatibility();
    dispatch_async(dispatch_get_main_queue(), ^{
        if (gPicker) {
            completion(request, nil, nil, @"picker_busy");
            return;
        }
        UIViewController *top = TopController();
        if (!top) {
            completion(request, nil, nil, @"picker_unavailable");
            return;
        }
        NSMutableArray<UTType *> *types = [NSMutableArray array];
        for (NSString *extension in extensions) {
            UTType *type = [UTType typeWithFilenameExtension:extension];
            if (type) [types addObject:type];
        }
        if (types.count == 0) [types addObjectsFromArray:@[ UTTypePlainText, UTTypeData ]];
        AstralFilePicker *pickerDelegate = [AstralFilePicker new];
        pickerDelegate.request = request;
        pickerDelegate.pluginCompletion = completion;
        gPicker = pickerDelegate;
        UIDocumentPickerViewController *picker = [[UIDocumentPickerViewController alloc] initForOpeningContentTypes:types];
        picker.delegate = pickerDelegate;
        pickerDelegate.controller = picker;
        [top presentViewController:picker animated:YES completion:nil];
    });
    return YES;
}

void AstralCancelPluginFile(uint64_t request)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!gPicker || gPicker.request != request) return;
        AstralFilePicker *picker = gPicker;
        gPicker = nil;
        picker.pluginCompletion = nil;
        [picker.controller dismissViewControllerAnimated:NO completion:nil];
    });
}

void AstralShowAlert(NSString *title, NSString *message)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIViewController *top = TopController();
        if (!top) return;
        UIAlertController *alert = [UIAlertController alertControllerWithTitle:title
                                                                       message:message
                                                                preferredStyle:UIAlertControllerStyleAlert];
        [alert addAction:[UIAlertAction actionWithTitle:@"OK" style:UIAlertActionStyleDefault handler:nil]];
        [top presentViewController:alert animated:YES completion:nil];
    });
}

void AstralShowToast(NSString *title, NSString *message)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIWindow *window = AstralPhoenixWindow();
        if (!window) return;
        [gToastView removeFromSuperview];
        UIView *container = [UIView new];
        container.translatesAutoresizingMaskIntoConstraints = NO;
        container.backgroundColor = [UIColor colorWithWhite:0.12 alpha:0.94];
        container.layer.cornerRadius = 14;
        container.layer.masksToBounds = YES;
        container.isAccessibilityElement = YES;
        container.accessibilityLabel = [NSString stringWithFormat:@"%@, %@", title, message];
        UILabel *titleLabel = [UILabel new];
        titleLabel.translatesAutoresizingMaskIntoConstraints = NO;
        titleLabel.text = title;
        titleLabel.textColor = UIColor.whiteColor;
        titleLabel.font = [UIFont preferredFontForTextStyle:UIFontTextStyleHeadline];
        titleLabel.numberOfLines = 1;
        UILabel *messageLabel = [UILabel new];
        messageLabel.translatesAutoresizingMaskIntoConstraints = NO;
        messageLabel.text = message;
        messageLabel.textColor = UIColor.whiteColor;
        messageLabel.font = [UIFont preferredFontForTextStyle:UIFontTextStyleSubheadline];
        messageLabel.numberOfLines = 5;
        messageLabel.lineBreakMode = NSLineBreakByTruncatingTail;
        UIStackView *stack = [[UIStackView alloc] initWithArrangedSubviews:@[ titleLabel, messageLabel ]];
        stack.translatesAutoresizingMaskIntoConstraints = NO;
        stack.axis = UILayoutConstraintAxisVertical;
        stack.spacing = 4;
        [container addSubview:stack];
        [window addSubview:container];
        UILayoutGuide *safe = window.safeAreaLayoutGuide;
        [NSLayoutConstraint activateConstraints:@[
            [container.leadingAnchor constraintGreaterThanOrEqualToAnchor:safe.leadingAnchor constant:16],
            [container.trailingAnchor constraintLessThanOrEqualToAnchor:safe.trailingAnchor constant:-16],
            [container.centerXAnchor constraintEqualToAnchor:safe.centerXAnchor],
            [container.bottomAnchor constraintEqualToAnchor:safe.bottomAnchor constant:-16],
            [container.widthAnchor constraintLessThanOrEqualToAnchor:safe.widthAnchor constant:-32],
            [stack.topAnchor constraintEqualToAnchor:container.topAnchor constant:12],
            [stack.bottomAnchor constraintEqualToAnchor:container.bottomAnchor constant:-12],
            [stack.leadingAnchor constraintEqualToAnchor:container.leadingAnchor constant:14],
            [stack.trailingAnchor constraintEqualToAnchor:container.trailingAnchor constant:-14],
        ]];
        gToastView = container;
        uint64_t generation = ++gToastGeneration;
        container.alpha = 0;
        [UIView animateWithDuration:0.2
                         animations:^{
                             container.alpha = 1;
                         }];
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(4 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
            if (generation != gToastGeneration) return;
            [UIView animateWithDuration:0.2
                animations:^{
                    container.alpha = 0;
                }
                completion:^(BOOL finished) {
                    [container removeFromSuperview];
                    if (gToastView == container) gToastView = nil;
                }];
        });
    });
}

void AstralEditText(NSString *title, NSString *value, NSUInteger maximumLength, void (^completion)(NSString *))
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIViewController *top = TopController();
        if (!top) {
            completion(nil);
            return;
        }
        AstralTextEditorController *editor = [AstralTextEditorController new];
        editor.titleText = title;
        editor.initialValue = value;
        editor.maximumLength = maximumLength;
        editor.completion = completion;
        editor.modalPresentationStyle = UIModalPresentationPageSheet;
        [top presentViewController:editor animated:YES completion:nil];
    });
}

static CGPoint gLastTouch;
static UIView *gTooltipView;

void AstralNoteTouch(CGPoint point)
{
    gLastTouch = point;
}

static UIView *TooltipBubble(NSString *text)
{
    UIView *bubble = [UIView new];
    bubble.backgroundColor = [UIColor colorWithRed:0.07 green:0.07 blue:0.08 alpha:0.98];
    bubble.layer.cornerRadius = 8;
    bubble.isAccessibilityElement = YES;
    bubble.accessibilityLabel = text;
    UILabel *label = [UILabel new];
    label.text = text;
    label.textColor = UIColor.whiteColor;
    label.font = [UIFont systemFontOfSize:14 weight:UIFontWeightSemibold];
    label.numberOfLines = 3;
    label.textAlignment = NSTextAlignmentCenter;
    [bubble addSubview:label];
    CGSize fitted = [label sizeThatFits:CGSizeMake(260, CGFLOAT_MAX)];
    label.frame = CGRectMake(12, 8, fitted.width, fitted.height);
    bubble.frame = CGRectMake(0, 0, fitted.width + 24, fitted.height + 16);
    return bubble;
}

void AstralShowTooltip(NSString *text)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIWindow *window = AstralPhoenixWindow();
        if (!window) return;
        [gTooltipView removeFromSuperview];
        UIView *bubble = TooltipBubble(text);
        CGSize size = bubble.frame.size;
        CGFloat inset = 8;
        CGFloat x = MIN(MAX(gLastTouch.x - size.width / 2, inset), window.bounds.size.width - size.width - inset);
        CGFloat above = gLastTouch.y - size.height - 18;
        CGFloat y = above >= window.safeAreaInsets.top + inset ? above : gLastTouch.y + 18;
        bubble.frame = CGRectMake(x, y, size.width, size.height);
        bubble.alpha = 0;
        bubble.transform = CGAffineTransformMakeScale(0.9, 0.9);
        [window addSubview:bubble];
        gTooltipView = bubble;
        [UIView animateWithDuration:0.15
                         animations:^{
                             bubble.alpha = 1;
                             bubble.transform = CGAffineTransformIdentity;
                         }];
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(2.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
            if (gTooltipView != bubble) return;
            [UIView animateWithDuration:0.2
                animations:^{
                    bubble.alpha = 0;
                }
                completion:^(__unused BOOL finished) {
                    [bubble removeFromSuperview];
                    if (gTooltipView == bubble) gTooltipView = nil;
                }];
        });
    });
}
