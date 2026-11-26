#import "LuaComposer.h"

#import "AstralCore.h"
#import <objc/runtime.h>
#import <UIKit/UIKit.h>

static const void *kComposerKey = &kComposerKey;
static NSString *const kBaseAttributesKey = @"astral.liveMarkdown.baseAttributes";
static NSHashTable<UITextView *> *gComposerViews;

static BOOL IsComposer(UITextView *textView)
{
    UIView *view = textView;
    for (NSUInteger depth = 0; view && depth < 32; depth++, view = view.superview) {
        if ([view.accessibilityIdentifier isEqualToString:@"chat-input-container"]) return YES;
    }
    return NO;
}

static NSUInteger UTF16Offset(NSData *utf8, NSUInteger byteOffset)
{
    if (byteOffset > utf8.length) return NSNotFound;
    NSData *prefix = [utf8 subdataWithRange:NSMakeRange(0, byteOffset)];
    NSString *decoded = [[NSString alloc] initWithData:prefix encoding:NSUTF8StringEncoding];
    return decoded ? decoded.length : NSNotFound;
}

static NSDictionary *BaseAttributes(NSAttributedString *storage, NSUInteger index)
{
    NSDictionary *attributes = [storage attributesAtIndex:index effectiveRange:nil];
    NSMutableDictionary *base = [NSMutableDictionary dictionary];
    for (NSAttributedStringKey key in @[
             NSFontAttributeName, NSForegroundColorAttributeName, NSBackgroundColorAttributeName, NSStrikethroughStyleAttributeName,
             NSUnderlineStyleAttributeName
         ]) {
        id value = attributes[key];
        if (value) base[key] = value;
    }
    return base;
}

static void RestoreBaseAttributes(NSTextStorage *storage)
{
    if (!storage.length) return;
    NSMutableArray<NSDictionary *> *saved = [NSMutableArray array];
    [storage enumerateAttribute:kBaseAttributesKey
                        inRange:NSMakeRange(0, storage.length)
                        options:0
                     usingBlock:^(NSDictionary *base, NSRange range, __unused BOOL *stop) {
                         if (base) [saved addObject:@{@"range" : [NSValue valueWithRange:range], @"base" : base}];
                     }];
    [storage beginEditing];
    for (NSDictionary *entry in saved) {
        NSRange range = [entry[@"range"] rangeValue];
        NSDictionary *base = entry[@"base"];
        for (NSAttributedStringKey key in @[
                 NSFontAttributeName, NSForegroundColorAttributeName, NSBackgroundColorAttributeName, NSStrikethroughStyleAttributeName,
                 NSUnderlineStyleAttributeName
             ]) {
            id value = base[key];
            if (value)
                [storage addAttribute:key value:value range:range];
            else
                [storage removeAttribute:key range:range];
        }
        [storage removeAttribute:kBaseAttributesKey range:range];
    }
    [storage endEditing];
}

static void StoreBaseAttributes(NSTextStorage *storage, NSRange range)
{
    NSUInteger cursor = range.location;
    NSUInteger end = NSMaxRange(range);
    while (cursor < end) {
        NSRange effective;
        NSDictionary *attributes = [storage attributesAtIndex:cursor effectiveRange:&effective];
        NSRange subrange = NSIntersectionRange(effective, range);
        if (!attributes[kBaseAttributesKey]) {
            [storage addAttribute:kBaseAttributesKey value:BaseAttributes(storage, cursor) range:subrange];
        }
        cursor = NSMaxRange(subrange);
    }
}

static UIFont *StyledFont(UIFont *base, NSString *style)
{
    CGFloat size = base.pointSize;
    if ([style isEqualToString:@"code"]) return [UIFont monospacedSystemFontOfSize:size * 0.9 weight:UIFontWeightRegular];
    CGFloat scale = 1;
    UIFontDescriptorSymbolicTraits traits = 0;
    if ([style isEqualToString:@"bold"]) traits = UIFontDescriptorTraitBold;
    if ([style isEqualToString:@"italic"]) traits = UIFontDescriptorTraitItalic;
    if ([style isEqualToString:@"heading1"]) scale = 1.45;
    if ([style isEqualToString:@"heading2"]) scale = 1.3;
    if ([style isEqualToString:@"heading3"]) scale = 1.15;
    if ([style isEqualToString:@"subtext"]) scale = 0.85;
    UIFontDescriptor *descriptor = base.fontDescriptor;
    if (traits) descriptor = [descriptor fontDescriptorWithSymbolicTraits:traits] ?: descriptor;
    return [UIFont fontWithDescriptor:descriptor size:size * scale] ?: base;
}

static void ApplyStyle(NSTextStorage *storage, UITextView *textView, NSString *style, NSRange range)
{
    if (!range.length || NSMaxRange(range) > storage.length) return;
    NSUInteger index = range.location;
    StoreBaseAttributes(storage, range);
    UIColor *color = [storage attribute:NSForegroundColorAttributeName atIndex:index effectiveRange:nil] ?: textView.textColor;
    UIFont *font = [storage attribute:NSFontAttributeName atIndex:index effectiveRange:nil] ?: textView.font;
    if ([style isEqualToString:@"marker"]) {
        [storage addAttribute:NSForegroundColorAttributeName value:[color colorWithAlphaComponent:0.4] range:range];
        return;
    }
    if ([style isEqualToString:@"quote"] || [style isEqualToString:@"subtext"]) {
        CGFloat alpha = [style isEqualToString:@"quote"] ? 0.75 : 0.65;
        [storage addAttribute:NSForegroundColorAttributeName value:[color colorWithAlphaComponent:alpha] range:range];
    }
    if ([style isEqualToString:@"strike"]) {
        [storage addAttribute:NSStrikethroughStyleAttributeName value:@(NSUnderlineStyleSingle) range:range];
        return;
    }
    if ([style isEqualToString:@"underline"]) {
        [storage addAttribute:NSUnderlineStyleAttributeName value:@(NSUnderlineStyleSingle) range:range];
        return;
    }
    if ([style isEqualToString:@"spoiler"]) {
        [storage addAttribute:NSBackgroundColorAttributeName value:[UIColor colorWithWhite:0 alpha:0.35] range:range];
        return;
    }
    if ([style isEqualToString:@"code"])
        [storage addAttribute:NSBackgroundColorAttributeName value:[UIColor colorWithWhite:0.5 alpha:0.2] range:range];
    UIFont *styled = StyledFont(font, style);
    if (styled) [storage addAttribute:NSFontAttributeName value:styled range:range];
}

static void Paint(UITextView *textView)
{
    if (!textView.window || textView.markedTextRange || !IsComposer(textView)) return;
    NSData *utf8 = [textView.text dataUsingEncoding:NSUTF8StringEncoding];
    if (!utf8) return;
    id styles = AstralTakeJSON(astral_ios_composer_styles(AstralTextOf(textView.text ?: @"")));
    NSArray<NSDictionary *> *runs = [styles isKindOfClass:NSArray.class] ? styles : @[];
    NSMutableArray<NSDictionary *> *styled = [NSMutableArray array];
    for (NSDictionary *run in runs) {
        NSUInteger start = [run[@"start"] unsignedIntegerValue];
        NSUInteger length = [run[@"length"] unsignedIntegerValue];
        NSString *style = [run[@"style"] isKindOfClass:NSString.class] ? run[@"style"] : nil;
        if (!start || !length || !style) continue;
        NSUInteger first = UTF16Offset(utf8, start - 1);
        NSUInteger end = UTF16Offset(utf8, start - 1 + length);
        if (first == NSNotFound || end == NSNotFound || end <= first || end > textView.textStorage.length) continue;
        [styled addObject:@{@"range" : [NSValue valueWithRange:NSMakeRange(first, end - first)], @"style" : style}];
    }
    NSString *key = [NSString stringWithFormat:@"%@\n%@", textView.text ?: @"", styled];
    if ([objc_getAssociatedObject(textView, kComposerKey) isEqual:key]) return;
    NSTextStorage *storage = textView.textStorage;
    RestoreBaseAttributes(storage);
    [storage beginEditing];
    for (NSDictionary *run in styled) ApplyStyle(storage, textView, run[@"style"], [run[@"range"] rangeValue]);
    [storage endEditing];
    objc_setAssociatedObject(textView, kComposerKey, key, OBJC_ASSOCIATION_COPY_NONATOMIC);
    [textView setNeedsDisplay];
}

void AstralRepaintLuaComposers(void)
{
    for (UITextView *textView in gComposerViews.allObjects) Paint(textView);
}

void AstralInstallLuaComposer(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gComposerViews = [NSHashTable weakObjectsHashTable];
        for (NSNotificationName name in @[ UITextViewTextDidChangeNotification, UITextViewTextDidBeginEditingNotification ]) {
            [NSNotificationCenter.defaultCenter addObserverForName:name
                                                            object:nil
                                                             queue:NSOperationQueue.mainQueue
                                                        usingBlock:^(NSNotification *notification) {
                                                            UITextView *textView = notification.object;
                                                            if (![textView isKindOfClass:UITextView.class] || !IsComposer(textView)) return;
                                                            [gComposerViews addObject:textView];
                                                            Paint(textView);
                                                        }];
        }
    });
}

static UITextView *FindComposer(UIView *view)
{
    if ([view isKindOfClass:UITextView.class] && !view.hidden && IsComposer((UITextView *)view)) return (UITextView *)view;
    for (UIView *child in view.subviews) {
        UITextView *found = FindComposer(child);
        if (found) return found;
    }
    return nil;
}

BOOL AstralInsertComposerText(NSString *text, BOOL replace)
{
    UITextView *composer = nil;
    for (UITextView *textView in gComposerViews.allObjects)
        if (textView.window && IsComposer(textView)) composer = textView;
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (composer || ![scene isKindOfClass:UIWindowScene.class]) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows) {
            composer = FindComposer(window);
            if (composer) break;
        }
    }
    if (!composer) return NO;
    if (!composer.isFirstResponder) [composer becomeFirstResponder];
    if (replace) composer.selectedRange = NSMakeRange(0, composer.text.length);
    if (text.length)
        [composer insertText:text];
    else if (replace && composer.text.length)
        [composer deleteBackward];
    return YES;
}
