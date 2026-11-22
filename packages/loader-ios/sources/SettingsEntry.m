#import "SettingsEntry.h"

#import "PhoenixImage.h"

@interface AstralSettingsEntry : UIButton
@property (nonatomic) UIEdgeInsets nativeInset;
@property (nonatomic) CGFloat reservedHeight;
@end

static void (*originalLayout)(UIScrollView *, SEL);
static AstralFallbackModel fallbackModel;
static AstralFallbackAction fallbackAction;
static const char entryKey = 0;

@implementation AstralSettingsEntry

- (instancetype)initWithFrame:(CGRect)frame
{
    self = [super initWithFrame:frame];
    if (!self) return nil;

    self.accessibilityLabel = @"astral";
    self.accessibilityHint = @"Opens astral settings";
    self.backgroundColor = UIColor.secondarySystemGroupedBackgroundColor;
    self.layer.cornerRadius = 12;
    self.autoresizingMask = UIViewAutoresizingFlexibleWidth;
    [self addTarget:self action:@selector(open) forControlEvents:UIControlEventTouchUpInside];

    UIImageView *icon = [[UIImageView alloc] initWithImage:[UIImage systemImageNamed:@"sparkles"]];
    icon.tintColor = UIColor.labelColor;
    icon.contentMode = UIViewContentModeScaleAspectFit;

    UILabel *title = [UILabel new];
    title.text = @"astral";
    title.font = [UIFont preferredFontForTextStyle:UIFontTextStyleBody];
    title.adjustsFontForContentSizeCategory = YES;

    UIImageView *chevron = [[UIImageView alloc] initWithImage:[UIImage systemImageNamed:@"chevron.forward"]];
    chevron.tintColor = UIColor.tertiaryLabelColor;
    chevron.contentMode = UIViewContentModeScaleAspectFit;

    UIStackView *stack = [[UIStackView alloc] initWithArrangedSubviews:@[ icon, title, chevron ]];
    stack.translatesAutoresizingMaskIntoConstraints = NO;
    stack.alignment = UIStackViewAlignmentCenter;
    stack.spacing = 16;
    stack.userInteractionEnabled = NO;
    stack.accessibilityElementsHidden = YES;
    [self addSubview:stack];
    [NSLayoutConstraint activateConstraints:@[
        [stack.leadingAnchor constraintEqualToAnchor:self.leadingAnchor constant:16],
        [stack.trailingAnchor constraintEqualToAnchor:self.trailingAnchor constant:-16],
        [stack.centerYAnchor constraintEqualToAnchor:self.centerYAnchor],
        [icon.widthAnchor constraintEqualToConstant:24],
        [icon.heightAnchor constraintEqualToConstant:24],
        [chevron.widthAnchor constraintEqualToConstant:12],
        [chevron.heightAnchor constraintEqualToConstant:16],
    ]];
    return self;
}

- (void)open
{
    UIViewController *presenter = self.window.rootViewController;
    while (presenter.presentedViewController && !presenter.presentedViewController.isBeingDismissed)
        presenter = presenter.presentedViewController;
    if (!presenter) return;
    UITableViewController *page =
        [(UITableViewController *)[NSClassFromString(@"AstralFallbackController") alloc] initWithStyle:UITableViewStyleInsetGrouped];
    UINavigationController *navigation = [[UINavigationController alloc] initWithRootViewController:page];
    [presenter presentViewController:navigation animated:YES completion:nil];
}

@end

@interface AstralFallbackController : UITableViewController
@end

@implementation AstralFallbackController {
    NSArray<NSDictionary *> *_sections;
}

- (void)viewDidLoad
{
    [super viewDidLoad];
    self.title = @"astral";
    self.navigationItem.rightBarButtonItem = [[UIBarButtonItem alloc] initWithBarButtonSystemItem:UIBarButtonSystemItemDone
                                                                                           target:self
                                                                                           action:@selector(close)];
    [self reload];
}

- (void)reload
{
    _sections = fallbackModel()[@"sections"];
    [self.tableView reloadData];
}

- (void)close
{
    [self dismissViewControllerAnimated:YES completion:nil];
}

- (NSInteger)numberOfSectionsInTableView:(UITableView *)tableView
{
    return _sections.count;
}

- (NSInteger)tableView:(UITableView *)tableView numberOfRowsInSection:(NSInteger)section
{
    return [_sections[section][@"rows"] count];
}

- (NSString *)tableView:(UITableView *)tableView titleForHeaderInSection:(NSInteger)section
{
    return _sections[section][@"title"];
}

- (UITableViewCell *)tableView:(UITableView *)tableView cellForRowAtIndexPath:(NSIndexPath *)index
{
    NSDictionary *row = _sections[index.section][@"rows"][index.row];
    UITableViewCell *cell = [[UITableViewCell alloc] initWithStyle:UITableViewCellStyleDefault reuseIdentifier:nil];
    cell.textLabel.text = row[@"title"];
    cell.selectionStyle = UITableViewCellSelectionStyleNone;
    if ([row[@"kind"] intValue] == 1) {
        UISwitch *toggle = [UISwitch new];
        toggle.on = [row[@"on"] boolValue];
        toggle.accessibilityIdentifier = row[@"key"];
        [toggle addTarget:self action:@selector(toggled:) forControlEvents:UIControlEventValueChanged];
        cell.accessoryView = toggle;
    }
    return cell;
}

- (void)toggled:(UISwitch *)toggle
{
    fallbackAction(toggle.accessibilityIdentifier);
    [self reload];
}

@end

static BOOL HasText(UIView *view, NSString *text, NSUInteger depth)
{
    if (view.hidden) return NO;
    if ([view isKindOfClass:UILabel.class] && [((UILabel *)view).text isEqualToString:text]) return YES;
    if (!depth) return NO;
    for (UIView *child in view.subviews) {
        if (HasText(child, text, depth - 1)) return YES;
    }
    return NO;
}

static BOOL IsSettingsOverview(UIScrollView *scroll)
{
    UIWindow *window = scroll.window;
    if (!window || scroll.hidden || scroll.bounds.size.width < 240 ||
        window.rootViewController.class != NSClassFromString(@"LibdiscordHost.RootController"))
        return NO;

    Class sheet = NSClassFromString(@"LibdiscordHost.PageSheetView");
    UIView *container = scroll.superview;
    UIView *ancestor = container;
    while (ancestor && ancestor.class != sheet && !ancestor.hidden) ancestor = ancestor.superview;
    if (!ancestor || ancestor.hidden) return NO;

    Class flex = NSClassFromString(@"LibdiscordHost.FlexBox");
    BOOL accountGroup = NO;
    for (UIView *content in scroll.subviews) {
        if (content.class != flex || content.hidden) continue;
        for (UIView *section in content.subviews) {
            if (section.class == flex && HasText(section, @"Account Settings", 1)) {
                accountGroup = YES;
                break;
            }
        }
        if (accountGroup) break;
    }
    if (!accountGroup) return NO;

    for (UIView *header in container.subviews) {
        if (header == scroll || CGRectGetMaxY(header.frame) > CGRectGetMinY(scroll.frame) + 1) continue;
        if (HasText(header, @"Settings", 4)) return YES;
    }
    return NO;
}

static UIEdgeInsets InsetWithEntry(AstralSettingsEntry *entry)
{
    UIEdgeInsets inset = entry.nativeInset;
    inset.top += entry.reservedHeight;
    return inset;
}

static void RemoveEntry(UIScrollView *scroll, AstralSettingsEntry *entry)
{
    objc_setAssociatedObject(scroll, &entryKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    [entry removeFromSuperview];
    if (UIEdgeInsetsEqualToEdgeInsets(scroll.contentInset, InsetWithEntry(entry))) {
        scroll.contentInset = entry.nativeInset;
    }
}

static void LayoutSettings(UIScrollView *scroll, SEL selector)
{
    originalLayout(scroll, selector);
    if (!NSThread.isMainThread) return;

    AstralSettingsEntry *entry = objc_getAssociatedObject(scroll, &entryKey);
    if (!IsSettingsOverview(scroll)) {
        if (entry) RemoveEntry(scroll, entry);
        return;
    }
    if (entry) {
        if (!UIEdgeInsetsEqualToEdgeInsets(scroll.contentInset, InsetWithEntry(entry))) RemoveEntry(scroll, entry);
        return;
    }
    if (scroll.contentInset.top != 0) return;

    const CGFloat margin = 16;
    const CGFloat height = 56;
    const CGFloat reserved = 72;
    BOOL atTop = fabs(scroll.contentOffset.y + scroll.adjustedContentInset.top) < 1;
    entry = [[AstralSettingsEntry alloc]
        initWithFrame:CGRectMake(margin, -reserved + margin / 2, CGRectGetWidth(scroll.bounds) - margin * 2, height)];
    entry.nativeInset = scroll.contentInset;
    entry.reservedHeight = reserved;
    objc_setAssociatedObject(scroll, &entryKey, entry, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    [scroll insertSubview:entry atIndex:0];
    scroll.contentInset = InsetWithEntry(entry);
    if (atTop) {
        [scroll setContentOffset:CGPointMake(scroll.contentOffset.x, -scroll.adjustedContentInset.top) animated:NO];
    }
}

BOOL AstralInstallSettingsEntry(AstralFallbackModel model, AstralFallbackAction action)
{
    if (originalLayout) return YES;
    Method layout = AstralPhoenixOwnedMethod(NSClassFromString(@"LibdiscordHost.ScrollBox"), @selector(layoutSubviews), "v16@0:8");
    if (!layout) return NO;

    fallbackModel = model;
    fallbackAction = action;
    originalLayout = (void *)method_getImplementation(layout);
    method_setImplementation(layout, (IMP)LayoutSettings);
    return YES;
}
