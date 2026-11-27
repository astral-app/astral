#import "PluginSheet.h"

#import <UIKit/UIKit.h>

#import "PhoenixImage.h"

static const CGFloat kInset = 16;
static const CGFloat kFillMinimumHeight = 140;
static const CGFloat kCornerRadius = 8;
static const NSUInteger kMaximumInput = 4000;

static NSMutableDictionary<NSNumber *, id> *gSheets;
static NSCache<NSString *, UIImage *> *gImages;

static UIFont *DiscordFont(CGFloat size, UIFontWeight weight)
{
    NSString *name = weight >= UIFontWeightBold ? @"ggsans-Bold" : weight >= UIFontWeightSemibold ? @"ggsans-Semibold" : @"ggsans-Normal";
    return [UIFont fontWithName:name size:size] ?: [UIFont systemFontOfSize:size weight:weight];
}

static UIFont *DiscordMonoFont(CGFloat size)
{
    return [UIFont fontWithName:@"ggmono-Normal" size:size] ?: [UIFont monospacedSystemFontOfSize:size weight:UIFontWeightRegular];
}

static UIColor *RGB(uint32_t rgb)
{
    return [UIColor colorWithRed:((rgb >> 16) & 0xFF) / 255.0 green:((rgb >> 8) & 0xFF) / 255.0 blue:(rgb & 0xFF) / 255.0 alpha:1];
}

static UIColor *Themed(uint32_t dark, uint32_t light)
{
    return [UIColor colorWithDynamicProvider:^UIColor *(UITraitCollection *traits) {
        return RGB(traits.userInterfaceStyle == UIUserInterfaceStyleLight ? light : dark);
    }];
}

static UIColor *SheetBackground(void)
{
    return Themed(0x232428, 0xF2F3F5);
}

static UIColor *SurfaceColor(void)
{
    return Themed(0x2B2D31, 0xFFFFFF);
}

static UIColor *CodeBackground(void)
{
    return Themed(0x1E1F22, 0xE3E5E8);
}

static UIColor *PrimaryText(void)
{
    return Themed(0xF2F3F5, 0x060607);
}

static UIColor *MutedText(void)
{
    return Themed(0xB5BAC1, 0x4E5058);
}

static UIColor *DividerColor(void)
{
    return Themed(0x3F4147, 0xD4D7DC);
}

static UIColor *ColorValue(id value, UIColor *fallback)
{
    if ([value isKindOfClass:NSNumber.class]) {
        uint32_t argb = [value unsignedIntValue];
        UIColor *color = RGB(argb & 0xFFFFFF);
        uint32_t alpha = argb >> 24;
        return alpha ? [color colorWithAlphaComponent:alpha / 255.0] : color;
    }
    if ([value isKindOfClass:NSString.class]) {
        NSString *text = value;
        if ([text isEqualToString:@"muted"]) return MutedText();
        if ([text isEqualToString:@"primary"]) return PrimaryText();
        if ([text isEqualToString:@"link"]) return Themed(0x00A8FC, 0x006CE7);
        if ([text isEqualToString:@"positive"]) return Themed(0x23A55A, 0x1F8B4C);
        if ([text isEqualToString:@"danger"]) return Themed(0xF23F43, 0xDA373C);
        if ([text hasPrefix:@"#"]) text = [text substringFromIndex:1];
        unsigned int parsed = 0;
        if ((text.length == 6 || text.length == 8) && [[NSScanner scannerWithString:text] scanHexInt:&parsed])
            return text.length == 6 ? RGB(parsed) : [RGB(parsed & 0xFFFFFF) colorWithAlphaComponent:(parsed >> 24) / 255.0];
    }
    return fallback;
}

static CGFloat NumberValue(NSDictionary *item, NSString *key, CGFloat fallback)
{
    id value = item[key];
    return [value isKindOfClass:NSNumber.class] ? [value doubleValue] : fallback;
}

static NSString *TextValue(NSDictionary *item, NSString *key)
{
    id value = item[key];
    if ([value isKindOfClass:NSString.class]) return value;
    if ([value isKindOfClass:NSNumber.class]) return [value stringValue];
    return @"";
}

static NSArray<NSDictionary *> *ItemList(id value)
{
    NSMutableArray *items = [NSMutableArray array];
    if (![value isKindOfClass:NSArray.class]) return items;
    for (id item in value)
        if ([item isKindOfClass:NSDictionary.class]) [items addObject:item];
    return items;
}

@interface AstralPluginSheet : UIViewController <UIAdaptivePresentationControllerDelegate, UITextViewDelegate>
@property (nonatomic) uint64_t sheet;
@property (nonatomic, copy) NSDictionary *spec;
@property (nonatomic, copy) AstralSheetEvent event;
@property (nonatomic) UIStackView *stack;
@property (nonatomic) UIStackView *headerStack;
@property (nonatomic) NSLayoutConstraint *headerGap;
@property (nonatomic) UILabel *titleLabel;
@property (nonatomic) UILabel *subtitleLabel;
@property (nonatomic) NSMutableDictionary<NSValue *, NSString *> *longActions;
@property (nonatomic) NSMutableDictionary<NSValue *, NSString *> *actions;
@property (nonatomic) NSMutableDictionary<NSValue *, UITextView *> *inputs;
@property (nonatomic) BOOL finished;
@property (nonatomic) NSLayoutConstraint *fillHeight;
@end

@implementation AstralPluginSheet

- (void)viewDidLoad
{
    [super viewDidLoad];
    self.view.backgroundColor = SheetBackground();
    self.titleLabel = [UILabel new];
    self.titleLabel.font = DiscordFont(18, UIFontWeightBold);
    self.titleLabel.textColor = PrimaryText();
    self.titleLabel.textAlignment = NSTextAlignmentCenter;
    self.titleLabel.numberOfLines = 2;
    self.titleLabel.translatesAutoresizingMaskIntoConstraints = NO;
    self.subtitleLabel = [UILabel new];
    self.subtitleLabel.font = DiscordFont(13, UIFontWeightMedium);
    self.subtitleLabel.textColor = MutedText();
    self.subtitleLabel.textAlignment = NSTextAlignmentCenter;
    self.subtitleLabel.translatesAutoresizingMaskIntoConstraints = NO;
    UIScrollView *scroll = [UIScrollView new];
    scroll.translatesAutoresizingMaskIntoConstraints = NO;
    scroll.alwaysBounceVertical = YES;
    scroll.keyboardDismissMode = UIScrollViewKeyboardDismissModeInteractive;
    self.stack = [UIStackView new];
    self.stack.axis = UILayoutConstraintAxisVertical;
    self.stack.spacing = 12;
    self.stack.translatesAutoresizingMaskIntoConstraints = NO;
    [scroll addSubview:self.stack];
    self.headerStack = [UIStackView new];
    self.headerStack.axis = UILayoutConstraintAxisVertical;
    self.headerStack.spacing = 12;
    self.headerStack.translatesAutoresizingMaskIntoConstraints = NO;
    [self.view addSubview:self.titleLabel];
    [self.view addSubview:self.subtitleLabel];
    [self.view addSubview:self.headerStack];
    [self.view addSubview:scroll];
    UILayoutGuide *safe = self.view.safeAreaLayoutGuide;
    [NSLayoutConstraint activateConstraints:@[
        [self.titleLabel.topAnchor constraintEqualToAnchor:safe.topAnchor constant:22],
        [self.titleLabel.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor constant:kInset],
        [self.titleLabel.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor constant:-kInset],
        [self.subtitleLabel.topAnchor constraintEqualToAnchor:self.titleLabel.bottomAnchor constant:2],
        [self.subtitleLabel.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor constant:kInset],
        [self.subtitleLabel.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor constant:-kInset],
        [self.headerStack.topAnchor constraintEqualToAnchor:self.subtitleLabel.bottomAnchor constant:14],
        [self.headerStack.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor constant:kInset],
        [self.headerStack.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor constant:-kInset],
        [scroll.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor],
        [scroll.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor],
        [scroll.bottomAnchor constraintEqualToAnchor:self.view.keyboardLayoutGuide.topAnchor],
        [self.stack.topAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.topAnchor],
        [self.stack.bottomAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.bottomAnchor constant:-kInset],
        [self.stack.leadingAnchor constraintEqualToAnchor:scroll.frameLayoutGuide.leadingAnchor constant:kInset],
        [self.stack.trailingAnchor constraintEqualToAnchor:scroll.frameLayoutGuide.trailingAnchor constant:-kInset],
    ]];
    self.headerGap = [scroll.topAnchor constraintEqualToAnchor:self.headerStack.bottomAnchor];
    self.headerGap.active = YES;
    self.fillHeight = [self.stack.heightAnchor constraintGreaterThanOrEqualToAnchor:scroll.frameLayoutGuide.heightAnchor constant:-kInset];
    [self render];
}

- (void)render
{
    self.actions = [NSMutableDictionary dictionary];
    self.inputs = [NSMutableDictionary dictionary];
    self.longActions = [NSMutableDictionary dictionary];
    self.titleLabel.text = TextValue(self.spec, @"title");
    self.subtitleLabel.text = TextValue(self.spec, @"subtitle");
    for (UIView *view in self.headerStack.arrangedSubviews) [view removeFromSuperview];
    for (NSDictionary *item in ItemList(self.spec[@"header"])) {
        UIView *view = [self viewForItem:item];
        if (view) [self.headerStack addArrangedSubview:view];
    }
    self.headerGap.constant = self.headerStack.arrangedSubviews.count ? 14 : 0;
    for (UIView *view in self.stack.arrangedSubviews) [view removeFromSuperview];
    BOOL fills = NO;
    for (NSDictionary *item in ItemList(self.spec[@"items"])) {
        UIView *view = [self viewForItem:item];
        if (view) [self.stack addArrangedSubview:view];
        fills = fills || [item[@"fill"] boolValue];
    }
    self.fillHeight.active = fills;
}

- (void)send:(NSString *)action value:(NSString *)value
{
    if (action.length) self.event(action, value ?: @"", NO);
}

- (void)finish
{
    if (self.finished) return;
    self.finished = YES;
    [gSheets removeObjectForKey:@(self.sheet)];
    self.event(@"", @"", YES);
}

- (void)presentationControllerDidDismiss:(__unused UIPresentationController *)presentationController
{
    [self finish];
}

- (void)viewDidDisappear:(BOOL)animated
{
    [super viewDidDisappear:animated];
    if (self.isBeingDismissed || !self.presentingViewController) [self finish];
}

- (void)tapped:(UITapGestureRecognizer *)recognizer
{
    [self send:self.actions[[NSValue valueWithNonretainedObject:recognizer.view]] value:@""];
}

- (void)held:(UILongPressGestureRecognizer *)recognizer
{
    if (recognizer.state != UIGestureRecognizerStateBegan) return;
    [self send:self.longActions[[NSValue valueWithNonretainedObject:recognizer.view]] value:@""];
}

- (void)pressed:(UIButton *)button
{
    [self send:self.actions[[NSValue valueWithNonretainedObject:button]] value:@""];
}

- (void)submitted:(UIButton *)button
{
    NSValue *key = [NSValue valueWithNonretainedObject:button];
    UITextView *input = self.inputs[key];
    NSString *text = [input.text stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    if (!text.length) return;
    [self send:self.actions[key] value:text];
    input.text = @"";
    [input resignFirstResponder];
}

- (BOOL)textView:(UITextView *)textView shouldChangeTextInRange:(NSRange)range replacementText:(NSString *)text
{
    return textView.text.length - range.length + text.length <= kMaximumInput;
}

- (void)attachAction:(NSDictionary *)item toView:(UIView *)view
{
    NSString *longAction = TextValue(item, @"long_action");
    if (longAction.length) {
        self.longActions[[NSValue valueWithNonretainedObject:view]] = longAction;
        view.userInteractionEnabled = YES;
        UILongPressGestureRecognizer *hold = [[UILongPressGestureRecognizer alloc] initWithTarget:self action:@selector(held:)];
        hold.minimumPressDuration = 0.35;
        [view addGestureRecognizer:hold];
    }
    NSString *action = TextValue(item, @"action");
    if (!action.length) return;
    self.actions[[NSValue valueWithNonretainedObject:view]] = action;
    view.userInteractionEnabled = YES;
    [view addGestureRecognizer:[[UITapGestureRecognizer alloc] initWithTarget:self action:@selector(tapped:)]];
}

- (UIView *)viewForItem:(NSDictionary *)item
{
    NSString *type = TextValue(item, @"type");
    UIView *view = nil;
    if ([type isEqualToString:@"heading"])
        view = [self label:item size:NumberValue(item, @"size", 13) weight:UIFontWeightBold color:MutedText()];
    else if ([type isEqualToString:@"text"])
        view = [self label:item
                      size:NumberValue(item, @"size", 15)
                    weight:[item[@"bold"] boolValue] ? UIFontWeightSemibold : UIFontWeightRegular
                     color:PrimaryText()];
    else if ([type isEqualToString:@"code"])
        view = [self code:item];
    else if ([type isEqualToString:@"image"])
        view = [self image:item];
    else if ([type isEqualToString:@"icon"])
        view = [self icon:item];
    else if ([type isEqualToString:@"row"] || [type isEqualToString:@"column"])
        view = [self group:item horizontal:[type isEqualToString:@"row"]];
    else if ([type isEqualToString:@"buttons"])
        view = [self buttons:item];
    else if ([type isEqualToString:@"input"])
        view = [self input:item];
    else if ([type isEqualToString:@"divider"])
        view = [self divider];
    else if ([type isEqualToString:@"spacer"])
        view = [self spacer:NumberValue(item, @"size", 8)];
    if (!view) return nil;
    if (item[@"background"]) view = [self card:view item:item];
    if (![type isEqualToString:@"buttons"] && ![type isEqualToString:@"input"]) [self attachAction:item toView:view];
    if (item[@"opacity"]) view.alpha = NumberValue(item, @"opacity", 1);
    return view;
}

- (UILabel *)label:(NSDictionary *)item size:(CGFloat)size weight:(UIFontWeight)weight color:(UIColor *)fallback
{
    UILabel *label = [UILabel new];
    label.text = TextValue(item, @"text");
    label.font = [item[@"mono"] boolValue] ? [UIFont monospacedSystemFontOfSize:size weight:weight] : DiscordFont(size, weight);
    label.textColor = ColorValue(item[@"color"], fallback);
    label.numberOfLines = (NSInteger)NumberValue(item, @"lines", 0);
    NSString *align = TextValue(item, @"align");
    label.textAlignment = [align isEqualToString:@"center"]
                              ? NSTextAlignmentCenter
                              : ([align isEqualToString:@"end"] ? NSTextAlignmentRight : NSTextAlignmentNatural);
    [label setContentCompressionResistancePriority:UILayoutPriorityDefaultLow forAxis:UILayoutConstraintAxisHorizontal];
    return label;
}

- (UIView *)code:(NSDictionary *)item
{
    UITextView *text = [UITextView new];
    text.text = TextValue(item, @"text");
    text.editable = NO;
    text.selectable = YES;
    text.font = DiscordMonoFont(NumberValue(item, @"size", 12));
    text.textColor = PrimaryText();
    text.backgroundColor = CodeBackground();
    text.layer.cornerRadius = kCornerRadius;
    text.textContainerInset = UIEdgeInsetsMake(10, 8, 10, 8);
    CGFloat height = NumberValue(item, @"height", 0);
    if ([item[@"fill"] boolValue]) {
        [text.heightAnchor constraintGreaterThanOrEqualToConstant:height > 0 ? height : kFillMinimumHeight].active = YES;
        [text setContentHuggingPriority:1 forAxis:UILayoutConstraintAxisVertical];
        [text setContentCompressionResistancePriority:UILayoutPriorityDefaultLow forAxis:UILayoutConstraintAxisVertical];
    } else if (height > 0) {
        [text.heightAnchor constraintEqualToConstant:height].active = YES;
    } else {
        text.scrollEnabled = NO;
    }
    return text;
}

- (UIView *)image:(NSDictionary *)item
{
    UIImageView *view = [UIImageView new];
    view.contentMode =
        [TextValue(item, @"fit") isEqualToString:@"cover"] ? UIViewContentModeScaleAspectFill : UIViewContentModeScaleAspectFit;
    view.clipsToBounds = YES;
    CGFloat size = NumberValue(item, @"size", 40);
    CGFloat width = NumberValue(item, @"width", size), height = NumberValue(item, @"height", size);
    [view.widthAnchor constraintEqualToConstant:width].active = YES;
    [view.heightAnchor constraintEqualToConstant:height].active = YES;
    view.layer.cornerRadius = [item[@"round"] boolValue] ? MIN(width, height) / 2 : NumberValue(item, @"radius", 0);
    NSString *address = TextValue(item, @"url");
    UIImage *cached = [gImages objectForKey:address];
    if (cached) {
        view.image = cached;
        return view;
    }
    NSURL *url = [address hasPrefix:@"https://"] ? [NSURL URLWithString:address] : nil;
    if (!url) return view;
    __weak UIImageView *weakView = view;
    [[NSURLSession.sharedSession dataTaskWithURL:url
                               completionHandler:^(NSData *data, __unused NSURLResponse *response, __unused NSError *error) {
                                   UIImage *image = data ? [UIImage imageWithData:data] : nil;
                                   if (!image) return;
                                   [gImages setObject:image forKey:address];
                                   dispatch_async(dispatch_get_main_queue(), ^{
                                       weakView.image = image;
                                   });
                               }] resume];
    return view;
}

- (UIView *)icon:(NSDictionary *)item
{
    static NSDictionary<NSString *, NSString *> *symbols;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        symbols = @{
            @"thumbs_up" : @"hand.thumbsup.fill",
            @"thumbs_down" : @"hand.thumbsdown.fill",
            @"star" : @"star.fill",
            @"chevron_right" : @"chevron.right",
            @"flag" : @"flag.fill",
            @"deny" : @"nosign",
            @"trash" : @"trash.fill",
            @"info" : @"info.circle.fill",
            @"link" : @"link",
        };
    });
    CGFloat size = NumberValue(item, @"size", 16);
    UIImageSymbolConfiguration *configuration = [UIImageSymbolConfiguration configurationWithPointSize:size
                                                                                                weight:UIImageSymbolWeightSemibold];
    UIImageView *view = [[UIImageView alloc] initWithImage:[UIImage systemImageNamed:symbols[TextValue(item, @"name")] ?: @"questionmark"
                                                                   withConfiguration:configuration]];
    view.tintColor = ColorValue(item[@"color"], MutedText());
    view.contentMode = UIViewContentModeCenter;
    [view.widthAnchor constraintEqualToConstant:size + 4].active = YES;
    [view.heightAnchor constraintEqualToConstant:size + 4].active = YES;
    return view;
}

- (UIView *)group:(NSDictionary *)item horizontal:(BOOL)horizontal
{
    UIStackView *stack = [UIStackView new];
    stack.axis = horizontal ? UILayoutConstraintAxisHorizontal : UILayoutConstraintAxisVertical;
    stack.spacing = NumberValue(item, @"spacing", horizontal ? 10 : 4);
    stack.alignment = horizontal
                          ? ([TextValue(item, @"align") isEqualToString:@"top"] ? UIStackViewAlignmentTop : UIStackViewAlignmentCenter)
                          : UIStackViewAlignmentFill;
    BOOL stretches = NO;
    for (NSDictionary *child in ItemList(item[@"items"])) {
        UIView *view = [self viewForItem:child];
        if (!view) continue;
        NSString *type = TextValue(child, @"type");
        stretches |= [type isEqualToString:@"column"] || [child[@"grow"] boolValue];
        if (horizontal && ![type isEqualToString:@"column"] && ![child[@"grow"] boolValue])
            [view setContentHuggingPriority:UILayoutPriorityRequired forAxis:UILayoutConstraintAxisHorizontal];
        [stack addArrangedSubview:view];
    }
    if (horizontal && !stretches) {
        UIView *spacer = [UIView new];
        [spacer setContentHuggingPriority:1 forAxis:UILayoutConstraintAxisHorizontal];
        [spacer setContentCompressionResistancePriority:1 forAxis:UILayoutConstraintAxisHorizontal];
        [stack addArrangedSubview:spacer];
    }
    return stack;
}

- (UIButton *)button:(NSDictionary *)item
{
    NSString *style = TextValue(item, @"style");
    UIButtonConfiguration *configuration = UIButtonConfiguration.filledButtonConfiguration;
    configuration.title = TextValue(item, @"label");
    configuration.cornerStyle = UIButtonConfigurationCornerStyleFixed;
    configuration.background.cornerRadius = kCornerRadius;
    configuration.baseForegroundColor = UIColor.whiteColor;
    configuration.baseBackgroundColor = [style isEqualToString:@"danger"]    ? RGB(0xDA373C)
                                        : [style isEqualToString:@"primary"] ? RGB(0x5865F2)
                                                                             : Themed(0x4E5058, 0x6D6F78);
    configuration.titleTextAttributesTransformer = ^NSDictionary *(NSDictionary *attributes) {
        NSMutableDictionary *updated = [attributes mutableCopy];
        updated[NSFontAttributeName] = DiscordFont(15, UIFontWeightSemibold);
        return updated;
    };
    UIButton *button = [UIButton buttonWithConfiguration:configuration primaryAction:nil];
    [button.heightAnchor constraintEqualToConstant:NumberValue(item, @"height", 40)].active = YES;
    self.actions[[NSValue valueWithNonretainedObject:button]] = TextValue(item, @"action");
    [button addTarget:self action:@selector(pressed:) forControlEvents:UIControlEventTouchUpInside];
    return button;
}

- (UIView *)buttons:(NSDictionary *)item
{
    UIStackView *stack = [UIStackView new];
    stack.axis =
        [TextValue(item, @"direction") isEqualToString:@"column"] ? UILayoutConstraintAxisVertical : UILayoutConstraintAxisHorizontal;
    stack.spacing = 8;
    stack.distribution = UIStackViewDistributionFillEqually;
    for (NSDictionary *child in ItemList(item[@"buttons"])) [stack addArrangedSubview:[self button:child]];
    return stack;
}

- (UIView *)labelledInput:(NSDictionary *)item
{
    UITextView *text = [UITextView new];
    text.font = DiscordFont(16, UIFontWeightRegular);
    text.textColor = PrimaryText();
    text.backgroundColor = SurfaceColor();
    text.layer.cornerRadius = kCornerRadius;
    text.textContainerInset = UIEdgeInsetsMake(12, 8, 12, 8);
    text.scrollEnabled = NO;
    text.delegate = self;
    text.text = TextValue(item, @"value");
    [text.heightAnchor constraintGreaterThanOrEqualToConstant:NumberValue(item, @"height", 96)].active = YES;
    UILabel *placeholder = [UILabel new];
    placeholder.text = TextValue(item, @"placeholder");
    placeholder.font = text.font;
    placeholder.textColor = MutedText();
    placeholder.translatesAutoresizingMaskIntoConstraints = NO;
    placeholder.hidden = text.text.length > 0;
    [text addSubview:placeholder];
    [NSLayoutConstraint activateConstraints:@[
        [placeholder.leadingAnchor constraintEqualToAnchor:text.leadingAnchor constant:13],
        [placeholder.topAnchor constraintEqualToAnchor:text.topAnchor constant:12],
    ]];
    [NSNotificationCenter.defaultCenter addObserverForName:UITextViewTextDidChangeNotification
                                                    object:text
                                                     queue:NSOperationQueue.mainQueue
                                                usingBlock:^(__unused NSNotification *note) {
                                                    placeholder.hidden = text.text.length > 0;
                                                }];
    UIButton *submit = [self button:@{@"label" : TextValue(item, @"submit_label"), @"style" : @"primary", @"action" : @""}];
    [submit removeTarget:self action:@selector(pressed:) forControlEvents:UIControlEventTouchUpInside];
    NSValue *key = [NSValue valueWithNonretainedObject:submit];
    self.actions[key] = TextValue(item, @"action");
    self.inputs[key] = text;
    [submit addTarget:self action:@selector(submitted:) forControlEvents:UIControlEventTouchUpInside];
    UIStackView *stack = [[UIStackView alloc] initWithArrangedSubviews:@[ text, submit ]];
    stack.axis = UILayoutConstraintAxisVertical;
    stack.spacing = 8;
    return stack;
}

- (UIView *)input:(NSDictionary *)item
{
    if (TextValue(item, @"submit_label").length) return [self labelledInput:item];
    UIView *container = [UIView new];
    container.backgroundColor = SurfaceColor();
    container.layer.cornerRadius = 20;
    UITextView *text = [UITextView new];
    text.font = DiscordFont(16, UIFontWeightRegular);
    text.textColor = PrimaryText();
    text.backgroundColor = UIColor.clearColor;
    text.scrollEnabled = NO;
    text.delegate = self;
    text.text = TextValue(item, @"value");
    text.translatesAutoresizingMaskIntoConstraints = NO;
    UILabel *placeholder = [UILabel new];
    placeholder.text = TextValue(item, @"placeholder");
    placeholder.font = text.font;
    placeholder.textColor = MutedText();
    placeholder.translatesAutoresizingMaskIntoConstraints = NO;
    placeholder.hidden = text.text.length > 0;
    [NSNotificationCenter.defaultCenter addObserverForName:UITextViewTextDidChangeNotification
                                                    object:text
                                                     queue:NSOperationQueue.mainQueue
                                                usingBlock:^(__unused NSNotification *note) {
                                                    placeholder.hidden = text.text.length > 0;
                                                }];
    UIButtonConfiguration *configuration = UIButtonConfiguration.filledButtonConfiguration;
    configuration.image = [UIImage systemImageNamed:@"paperplane.fill"];
    configuration.cornerStyle = UIButtonConfigurationCornerStyleCapsule;
    configuration.baseBackgroundColor = RGB(0x5865F2);
    configuration.baseForegroundColor = UIColor.whiteColor;
    UIButton *send = [UIButton buttonWithConfiguration:configuration primaryAction:nil];
    send.translatesAutoresizingMaskIntoConstraints = NO;
    NSValue *key = [NSValue valueWithNonretainedObject:send];
    self.actions[key] = TextValue(item, @"action");
    self.inputs[key] = text;
    [send addTarget:self action:@selector(submitted:) forControlEvents:UIControlEventTouchUpInside];
    [container addSubview:text];
    [container addSubview:placeholder];
    [container addSubview:send];
    [NSLayoutConstraint activateConstraints:@[
        [text.topAnchor constraintEqualToAnchor:container.topAnchor constant:4],
        [text.bottomAnchor constraintEqualToAnchor:container.bottomAnchor constant:-4],
        [text.leadingAnchor constraintEqualToAnchor:container.leadingAnchor constant:10],
        [text.trailingAnchor constraintEqualToAnchor:send.leadingAnchor constant:-6],
        [text.heightAnchor constraintGreaterThanOrEqualToConstant:36],
        [placeholder.leadingAnchor constraintEqualToAnchor:text.leadingAnchor constant:5],
        [placeholder.centerYAnchor constraintEqualToAnchor:text.centerYAnchor],
        [send.trailingAnchor constraintEqualToAnchor:container.trailingAnchor constant:-6],
        [send.bottomAnchor constraintEqualToAnchor:container.bottomAnchor constant:-6],
        [send.widthAnchor constraintEqualToConstant:32],
        [send.heightAnchor constraintEqualToConstant:32],
    ]];
    return container;
}

- (UIView *)divider
{
    UIView *line = [UIView new];
    line.backgroundColor = DividerColor();
    [line.heightAnchor constraintEqualToConstant:1 / UIScreen.mainScreen.scale].active = YES;
    return line;
}

- (UIView *)spacer:(CGFloat)size
{
    UIView *space = [UIView new];
    [space.heightAnchor constraintEqualToConstant:size].active = YES;
    return space;
}

- (UIView *)card:(UIView *)content item:(NSDictionary *)item
{
    UIView *card = [UIView new];
    card.backgroundColor = ColorValue(item[@"background"], SurfaceColor());
    card.layer.cornerRadius = NumberValue(item, @"radius", kCornerRadius);
    CGFloat padding = NumberValue(item, @"padding", 12);
    content.translatesAutoresizingMaskIntoConstraints = NO;
    [card addSubview:content];
    [NSLayoutConstraint activateConstraints:@[
        [content.topAnchor constraintEqualToAnchor:card.topAnchor constant:padding],
        [content.bottomAnchor constraintEqualToAnchor:card.bottomAnchor constant:-padding],
        [content.leadingAnchor constraintEqualToAnchor:card.leadingAnchor constant:padding],
        [content.trailingAnchor constraintEqualToAnchor:card.trailingAnchor constant:-padding],
    ]];
    return card;
}

@end

static UIViewController *Presenter(void)
{
    UIViewController *top = AstralPhoenixWindow().rootViewController;
    while (top.presentedViewController && !top.presentedViewController.isBeingDismissed) top = top.presentedViewController;
    return top;
}

BOOL AstralOpenSheet(uint64_t sheet, NSDictionary *spec, AstralSheetEvent event)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!gSheets) gSheets = [NSMutableDictionary dictionary];
        if (!gImages) gImages = [NSCache new];
        UIViewController *presenter = Presenter();
        if (!presenter) {
            event(@"", @"", YES);
            return;
        }
        AstralPluginSheet *controller = [AstralPluginSheet new];
        controller.sheet = sheet;
        controller.spec = spec;
        controller.event = event;
        controller.modalPresentationStyle = UIModalPresentationPageSheet;
        UISheetPresentationController *presentation = controller.sheetPresentationController;
        BOOL expanded = [spec[@"expanded"] boolValue];
        presentation.detents = expanded
                                   ? @[ UISheetPresentationControllerDetent.largeDetent ]
                                   : @[ UISheetPresentationControllerDetent.mediumDetent, UISheetPresentationControllerDetent.largeDetent ];
        presentation.prefersGrabberVisible = YES;
        presentation.preferredCornerRadius = 16;
        controller.presentationController.delegate = controller;
        gSheets[@(sheet)] = controller;
        [presenter presentViewController:controller animated:YES completion:nil];
    });
    return YES;
}

void AstralUpdateSheet(uint64_t sheet, NSDictionary *spec)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralPluginSheet *controller = gSheets[@(sheet)];
        if (!controller) return;
        controller.spec = spec;
        if (controller.isViewLoaded) [controller render];
    });
}

void AstralCloseSheet(uint64_t sheet)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralPluginSheet *controller = gSheets[@(sheet)];
        if (!controller) return;
        [controller dismissViewControllerAnimated:YES
                                       completion:^{
                                           [controller finish];
                                       }];
    });
}
