#import "EmojiComposer.h"

#import <CommonCrypto/CommonDigest.h>
#import <ImageIO/ImageIO.h>
#import <UIKit/UIKit.h>
#import <objc/runtime.h>

#import "AstralCore.h"
#import "EmojiPacks.h"

static const NSInteger kOverlayTag = 0x55454d4a;
static const NSUInteger kMaxEmoji = 64;
static const char kHiddenRangesKey = 0;

static NSCache<NSString *, UIImage *> *gImages;
static NSMutableSet<NSString *> *gLoading;
static NSHashTable<UITextView *> *gTextViews;

static NSString *CacheDirectory(void)
{
    NSString *caches = NSSearchPathForDirectoriesInDomains(NSCachesDirectory, NSUserDomainMask, YES).firstObject;
    return [caches stringByAppendingPathComponent:@"astral/emoji"];
}

static NSString *CachePath(NSString *url)
{
    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    NSData *data = [url dataUsingEncoding:NSUTF8StringEncoding];
    CC_SHA256(data.bytes, (CC_LONG)data.length, digest);
    NSMutableString *name = [NSMutableString string];
    for (int index = 0; index < 16; index++) [name appendFormat:@"%02x", digest[index]];
    return [CacheDirectory() stringByAppendingPathComponent:name];
}

static const NSUInteger kMaxFrames = 120;
static const CGFloat kOverlayPixels = 96;

static double FrameDelay(CGImageSourceRef source, size_t index)
{
    NSDictionary *properties = CFBridgingRelease(CGImageSourceCopyPropertiesAtIndex(source, index, NULL));
    for (NSString *container in
         @[ (__bridge NSString *)kCGImagePropertyWebPDictionary, (__bridge NSString *)kCGImagePropertyGIFDictionary ]) {
        NSDictionary *frame = properties[container];
        NSNumber *delay = frame[(__bridge NSString *)kCGImagePropertyWebPUnclampedDelayTime] ?: frame[(__bridge NSString *)kCGImagePropertyWebPDelayTime]
                          ?: frame[(__bridge NSString *)kCGImagePropertyGIFUnclampedDelayTime] ?: frame[(__bridge NSString *)kCGImagePropertyGIFDelayTime];
        if (delay.doubleValue > 0.01) return delay.doubleValue;
    }
    return 0.05;
}

static UIImage *DecodedImage(NSData *data)
{
    CGImageSourceRef source = CGImageSourceCreateWithData((__bridge CFDataRef)data, NULL);
    if (!source) return nil;
    size_t count = MIN(CGImageSourceGetCount(source), kMaxFrames);
    NSDictionary *options = @{
        (__bridge NSString *)kCGImageSourceCreateThumbnailFromImageAlways : @YES,
        (__bridge NSString *)kCGImageSourceThumbnailMaxPixelSize : @(kOverlayPixels),
        (__bridge NSString *)kCGImageSourceCreateThumbnailWithTransform : @YES,
    };
    NSMutableArray<UIImage *> *frames = [NSMutableArray array];
    double duration = 0;
    for (size_t index = 0; index < count; index++) {
        CGImageRef frame = CGImageSourceCreateThumbnailAtIndex(source, index, (__bridge CFDictionaryRef)options);
        if (!frame) continue;
        [frames addObject:[UIImage imageWithCGImage:frame scale:UIScreen.mainScreen.scale orientation:UIImageOrientationUp]];
        CGImageRelease(frame);
        duration += FrameDelay(source, index);
    }
    CFRelease(source);
    if (frames.count > 1) return [UIImage animatedImageWithImages:frames duration:duration];
    return frames.firstObject;
}

static UIImage *ImageFor(NSString *url)
{
    UIImage *cached = [gImages objectForKey:url];
    if (cached) return cached;
    NSData *data = [NSData dataWithContentsOfFile:CachePath(url)];
    UIImage *image = data ? DecodedImage(data) : nil;
    if (image) {
        [gImages setObject:image forKey:url];
        return image;
    }
    if ([gLoading containsObject:url]) return nil;
    [gLoading addObject:url];
    [[NSURLSession.sharedSession dataTaskWithURL:[NSURL URLWithString:url]
                               completionHandler:^(NSData *body, NSURLResponse *response, NSError *error) {
                                   NSInteger status =
                                       [response isKindOfClass:NSHTTPURLResponse.class] ? ((NSHTTPURLResponse *)response).statusCode : 0;
                                   if (!error && status == 200 && body.length) {
                                       [NSFileManager.defaultManager createDirectoryAtPath:CacheDirectory()
                                                               withIntermediateDirectories:YES
                                                                                attributes:nil
                                                                                     error:nil];
                                       [body writeToFile:CachePath(url) atomically:YES];
                                   }
                                   dispatch_async(dispatch_get_main_queue(), ^{
                                       [gLoading removeObject:url];
                                       AstralRepaintEmojiComposers();
                                   });
                               }] resume];
    return nil;
}

static NSString *Codepoints(NSString *sequence)
{
    NSMutableArray *points = [NSMutableArray array];
    NSUInteger index = 0;
    while (index < sequence.length) {
        UTF32Char point = 0;
        unichar high = [sequence characterAtIndex:index];
        if (CFStringIsSurrogateHighCharacter(high) && index + 1 < sequence.length) {
            point = CFStringGetLongCharacterForSurrogatePair(high, [sequence characterAtIndex:index + 1]);
            index += 2;
        } else {
            point = high;
            index += 1;
        }
        [points addObject:[NSString stringWithFormat:@"%x", (unsigned)point]];
    }
    return [points componentsJoinedByString:@"-"];
}

static void RemoveOverlays(UITextView *textView)
{
    for (UIView *view in textView.subviews.copy) {
        if (view.tag == kOverlayTag) [view removeFromSuperview];
    }
}

static void (*gOriginalDrawGlyphs)(NSLayoutManager *, SEL, NSRange, CGPoint);

static void DrawGlyphs(NSLayoutManager *self, SEL selector, NSRange glyphs, CGPoint origin)
{
    NSArray<NSValue *> *hidden = objc_getAssociatedObject(self, &kHiddenRangesKey);
    if (!hidden.count) {
        gOriginalDrawGlyphs(self, selector, glyphs, origin);
        return;
    }
    NSUInteger cursor = glyphs.location, end = NSMaxRange(glyphs), length = self.textStorage.length;
    for (NSValue *value in hidden) {
        if (NSMaxRange(value.rangeValue) > length) continue;
        NSRange skip = NSIntersectionRange([self glyphRangeForCharacterRange:value.rangeValue actualCharacterRange:NULL], glyphs);
        if (!skip.length || skip.location < cursor) continue;
        if (skip.location > cursor) gOriginalDrawGlyphs(self, selector, NSMakeRange(cursor, skip.location - cursor), origin);
        cursor = NSMaxRange(skip);
    }
    if (cursor < end) gOriginalDrawGlyphs(self, selector, NSMakeRange(cursor, end - cursor), origin);
}

static void HideRanges(UITextView *textView, NSArray<NSValue *> *ranges)
{
    NSLayoutManager *layout = textView.layoutManager;
    NSArray *previous = objc_getAssociatedObject(layout, &kHiddenRangesKey);
    if (!previous.count && !ranges.count) return;
    NSArray *sorted = [ranges sortedArrayUsingComparator:^NSComparisonResult(NSValue *first, NSValue *second) {
        return first.rangeValue.location < second.rangeValue.location ? NSOrderedAscending : NSOrderedDescending;
    }];
    objc_setAssociatedObject(layout, &kHiddenRangesKey, sorted.count ? sorted : nil, OBJC_ASSOCIATION_COPY_NONATOMIC);
    [layout invalidateDisplayForCharacterRange:NSMakeRange(0, textView.textStorage.length)];
}

static CGRect OverlayFrame(UITextView *textView, NSRange characters, CGFloat side)
{
    NSLayoutManager *layout = textView.layoutManager;
    NSTextContainer *container = textView.textContainer;
    [layout ensureLayoutForTextContainer:container];
    NSRange glyphs = [layout glyphRangeForCharacterRange:characters actualCharacterRange:NULL];
    if (!glyphs.length) return CGRectNull;
    CGRect line = [layout lineFragmentUsedRectForGlyphAtIndex:glyphs.location effectiveRange:NULL];
    CGRect at = [layout boundingRectForGlyphRange:glyphs inTextContainer:container];
    UIEdgeInsets inset = textView.textContainerInset;
    if (line.size.height <= 0 || at.size.width <= 0) return CGRectNull;
    CGFloat size = MIN(side, MAX(at.size.width, side * 0.8));
    return CGRectMake(at.origin.x + inset.left + (at.size.width - size) / 2, line.origin.y + inset.top + (line.size.height - size) / 2,
                      size, size);
}

static void Paint(UITextView *textView);

static void PaintWhenShown(UITextView *textView, int attempts)
{
    if (attempts <= 0) return;
    __weak UITextView *weakView = textView;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(0.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        UITextView *strongView = weakView;
        if (!strongView) return;
        if (strongView.window)
            Paint(strongView);
        else
            PaintWhenShown(strongView, attempts - 1);
    });
}

static void Paint(UITextView *textView)
{
    if (!textView.window) {
        PaintWhenShown(textView, 20);
        return;
    }
    if (textView.markedTextRange) return;
    RemoveOverlays(textView);
    NSString *text = textView.text;
    if (!text.length || !astral_ios_custom_emoji_pack()) {
        HideRanges(textView, @[]);
        return;
    }
    CGFloat side = ceil(textView.font.lineHeight ?: 20);
    NSMutableArray<NSDictionary *> *runs = [NSMutableArray array];
    [text enumerateSubstringsInRange:NSMakeRange(0, text.length)
                             options:NSStringEnumerationByComposedCharacterSequences
                          usingBlock:^(NSString *sequence, NSRange range, __unused NSRange enclosing, BOOL *stop) {
                              if (runs.count >= kMaxEmoji) {
                                  *stop = YES;
                                  return;
                              }
                              if (range.length < 1 || [sequence characterAtIndex:0] < 0x2000) return;
                              NSString *url = AstralEmojiURL(Codepoints(sequence));
                              UIImage *image = url ? ImageFor(url) : nil;
                              if (image) [runs addObject:@{@"range" : [NSValue valueWithRange:range], @"image" : image}];
                          }];
    NSMutableArray<NSValue *> *hidden = [NSMutableArray array];
    for (NSDictionary *run in runs) {
        CGRect frame = OverlayFrame(textView, [run[@"range"] rangeValue], side);
        if (CGRectIsNull(frame)) continue;
        UIImageView *overlay = [[UIImageView alloc] initWithFrame:frame];
        overlay.image = run[@"image"];
        overlay.contentMode = UIViewContentModeScaleAspectFit;
        overlay.userInteractionEnabled = NO;
        overlay.tag = kOverlayTag;
        [textView addSubview:overlay];
        [hidden addObject:run[@"range"]];
    }
    HideRanges(textView, hidden);
}

static UITextView *FindIn(UIView *view, NSTextStorage *storage, int depth)
{
    if ([view isKindOfClass:UITextView.class] && ((UITextView *)view).textStorage == storage) return (UITextView *)view;
    if (depth > 40) return nil;
    for (UIView *child in view.subviews) {
        UITextView *found = FindIn(child, storage, depth + 1);
        if (found) return found;
    }
    return nil;
}

static UITextView *TextViewFor(NSTextStorage *storage)
{
    if (!storage) return nil;
    for (UITextView *known in gTextViews.allObjects) {
        if (known.textStorage == storage) return known;
    }
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class]) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows) {
            UITextView *found = FindIn(window, storage, 0);
            if (found) return found;
        }
    }
    return nil;
}

static void PaintStorage(NSTextStorage *storage, int attempts)
{
    __weak NSTextStorage *weakStorage = storage;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, attempts == 20 ? 0 : (int64_t)(0.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        NSTextStorage *strongStorage = weakStorage;
        if (!strongStorage) return;
        UITextView *textView = TextViewFor(strongStorage);
        if (!textView) {
            if (attempts > 1) PaintStorage(strongStorage, attempts - 1);
            return;
        }
        [gTextViews addObject:textView];
        Paint(textView);
    });
}

void AstralRepaintEmojiComposers(void)
{
    for (UITextView *textView in gTextViews.allObjects) Paint(textView);
}

static void (*gOriginalDidMoveToWindow)(UITextView *, SEL);

static void DidMoveToWindow(UITextView *self, SEL selector)
{
    gOriginalDidMoveToWindow(self, selector);
    if (!self.window || !self.text.length) return;
    [gTextViews addObject:self];
    __weak UITextView *weakView = self;
    dispatch_async(dispatch_get_main_queue(), ^{
        UITextView *strongView = weakView;
        if (strongView) Paint(strongView);
    });
}

static void HookGlyphDrawing(void)
{
    Method method = class_getInstanceMethod(NSLayoutManager.class, @selector(drawGlyphsForGlyphRange:atPoint:));
    if (method) gOriginalDrawGlyphs = (void (*)(NSLayoutManager *, SEL, NSRange, CGPoint))method_setImplementation(method, (IMP)DrawGlyphs);
}

static void HookWindowMoves(void)
{
    Method method = class_getInstanceMethod(UITextView.class, @selector(didMoveToWindow));
    if (!method) return;
    if (class_addMethod(UITextView.class, @selector(didMoveToWindow), (IMP)DidMoveToWindow, method_getTypeEncoding(method))) {
        gOriginalDidMoveToWindow = (void (*)(UITextView *, SEL))method_getImplementation(method);
        return;
    }
    gOriginalDidMoveToWindow = (void (*)(UITextView *, SEL))method_setImplementation(method, (IMP)DidMoveToWindow);
}

void AstralInstallEmojiComposer(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        HookWindowMoves();
        HookGlyphDrawing();
        gImages = [NSCache new];
        gLoading = [NSMutableSet set];
        gTextViews = [NSHashTable weakObjectsHashTable];
        [NSNotificationCenter.defaultCenter
            addObserverForName:NSTextStorageDidProcessEditingNotification
                        object:nil
                         queue:nil
                    usingBlock:^(NSNotification *notification) {
                        NSTextStorage *storage = notification.object;
                        if (![storage isKindOfClass:NSTextStorage.class] || !(storage.editedMask & NSTextStorageEditedCharacters)) return;
                        PaintStorage(storage, 20);
                    }];
        [NSNotificationCenter.defaultCenter addObserverForName:UITextViewTextDidChangeNotification
                                                        object:nil
                                                         queue:NSOperationQueue.mainQueue
                                                    usingBlock:^(NSNotification *notification) {
                                                        UITextView *textView = notification.object;
                                                        if (![textView isKindOfClass:UITextView.class]) return;
                                                        [gTextViews addObject:textView];
                                                        Paint(textView);
                                                    }];
    });
}
