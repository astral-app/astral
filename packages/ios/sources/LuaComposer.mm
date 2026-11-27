#import "LuaComposer.h"

#import "AstralCore.h"
#import <objc/runtime.h>
#import <UIKit/UIKit.h>

static const void *kComposerKey = &kComposerKey;
static NSString *const kBaseAttributesKey = @"astral.liveMarkdown.baseAttributes";
static NSHashTable<UITextView *> *gComposerViews;
static NSCache<NSString *, UIImage *> *gRunImages;
static NSMutableSet<NSString *> *gLoadingImages;
static const NSInteger kImageRunTag = 0x41494D47;

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
             NSUnderlineStyleAttributeName, NSKernAttributeName
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
                 NSUnderlineStyleAttributeName, NSKernAttributeName
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

static CGFloat ImageSide(UITextView *textView)
{
    return round((textView.font ?: [UIFont systemFontOfSize:16]).pointSize * 1.375);
}

static void HideRun(NSTextStorage *storage, UITextView *textView, NSRange range)
{
    if (!range.length || NSMaxRange(range) > storage.length) return;
    StoreBaseAttributes(storage, range);
    UIFont *tiny = [UIFont systemFontOfSize:0.01];
    CGFloat side = ImageSide(textView);
    CGFloat glyphs = [[storage.string substringWithRange:range] sizeWithAttributes:@{NSFontAttributeName : tiny}].width;
    [storage addAttribute:NSForegroundColorAttributeName value:UIColor.clearColor range:range];
    [storage addAttribute:NSFontAttributeName value:tiny range:range];
    [storage addAttribute:NSKernAttributeName value:@(MAX(side + 2 - glyphs, 0)) range:NSMakeRange(NSMaxRange(range) - 1, 1)];
}

static void Paint(UITextView *textView);

static void LoadRunImage(NSString *url)
{
    if ([gRunImages objectForKey:url] || [gLoadingImages containsObject:url]) return;
    NSURL *link = [NSURL URLWithString:url];
    if (!link) return;
    [gLoadingImages addObject:url];
    [[NSURLSession.sharedSession dataTaskWithURL:link
                               completionHandler:^(NSData *data, __unused NSURLResponse *response, __unused NSError *error) {
                                   UIImage *image = data ? [UIImage imageWithData:data] : nil;
                                   dispatch_async(dispatch_get_main_queue(), ^{
                                       [gLoadingImages removeObject:url];
                                       if (!image) return;
                                       [gRunImages setObject:image forKey:url];
                                       for (UITextView *textView in gComposerViews.allObjects) {
                                           objc_setAssociatedObject(textView, kComposerKey, nil, OBJC_ASSOCIATION_COPY_NONATOMIC);
                                           Paint(textView);
                                       }
                                   });
                               }] resume];
}

static void PlaceRunImages(UITextView *textView, NSArray<NSDictionary *> *images)
{
    for (UIView *view in textView.subviews.copy)
        if (view.tag == kImageRunTag) [view removeFromSuperview];
    if (!images.count) return;
    NSLayoutManager *layout = textView.layoutManager;
    NSTextContainer *container = textView.textContainer;
    [layout ensureLayoutForTextContainer:container];
    CGFloat side = ImageSide(textView);
    UIEdgeInsets inset = textView.textContainerInset;
    for (NSDictionary *run in images) {
        UIImage *image = [gRunImages objectForKey:run[@"image"]];
        if (!image) {
            LoadRunImage(run[@"image"]);
            continue;
        }
        NSRange glyphs = [layout glyphRangeForCharacterRange:[run[@"range"] rangeValue] actualCharacterRange:nil];
        if (!glyphs.length) continue;
        CGRect bounds = [layout boundingRectForGlyphRange:glyphs inTextContainer:container];
        CGRect line = [layout lineFragmentRectForGlyphAtIndex:glyphs.location effectiveRange:nil];
        UIImageView *view = [[UIImageView alloc] initWithImage:image];
        view.tag = kImageRunTag;
        view.contentMode = UIViewContentModeScaleAspectFit;
        view.userInteractionEnabled = NO;
        view.frame = CGRectMake(inset.left + bounds.origin.x + 1, inset.top + CGRectGetMidY(line) - side / 2, side, side);
        [textView addSubview:view];
    }
}

static void Paint(UITextView *textView)
{
    if (!textView.window || textView.markedTextRange || !IsComposer(textView)) return;
    NSData *utf8 = [textView.text dataUsingEncoding:NSUTF8StringEncoding];
    if (!utf8) return;
    id styles = AstralTakeJSON(astral_ios_composer_styles(AstralTextOf(textView.text ?: @"")));
    NSArray<NSDictionary *> *runs = [styles isKindOfClass:NSArray.class] ? styles : @[];
    NSMutableArray<NSDictionary *> *styled = [NSMutableArray array];
    NSMutableArray<NSDictionary *> *images = [NSMutableArray array];
    for (NSDictionary *run in runs) {
        NSUInteger start = [run[@"start"] unsignedIntegerValue];
        NSUInteger length = [run[@"length"] unsignedIntegerValue];
        NSString *style = [run[@"style"] isKindOfClass:NSString.class] ? run[@"style"] : nil;
        if (!start || !length || !style) continue;
        NSUInteger first = UTF16Offset(utf8, start - 1);
        NSUInteger end = UTF16Offset(utf8, start - 1 + length);
        if (first == NSNotFound || end == NSNotFound || end <= first || end > textView.textStorage.length) continue;
        NSValue *range = [NSValue valueWithRange:NSMakeRange(first, end - first)];
        NSString *image = [run[@"image"] isKindOfClass:NSString.class] ? run[@"image"] : nil;
        if ([style isEqualToString:@"image"] && image.length)
            [images addObject:@{@"range" : range, @"image" : image}];
        else
            [styled addObject:@{@"range" : range, @"style" : style}];
    }
    NSString *key = [NSString stringWithFormat:@"%@\n%@\n%@", textView.text ?: @"", styled, images];
    if ([objc_getAssociatedObject(textView, kComposerKey) isEqual:key]) return;
    NSTextStorage *storage = textView.textStorage;
    RestoreBaseAttributes(storage);
    [storage beginEditing];
    for (NSDictionary *run in styled) ApplyStyle(storage, textView, run[@"style"], [run[@"range"] rangeValue]);
    for (NSDictionary *run in images)
        if ([gRunImages objectForKey:run[@"image"]]) HideRun(storage, textView, [run[@"range"] rangeValue]);
    [storage endEditing];
    PlaceRunImages(textView, images);
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
        gRunImages = [NSCache new];
        gLoadingImages = [NSMutableSet set];
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
