#import "EmojiComposer.h"

#import <CommonCrypto/CommonDigest.h>
#import <UIKit/UIKit.h>

#import "EmojiPacks.h"

static const NSInteger kOverlayTag = 0x55454d4a;
static const CGFloat kHiddenFontSize = 0.01;
static const NSUInteger kMaxEmoji = 64;
static NSString *const kHiddenAttribute = @"app.astral.emoji-hidden";

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

static UIImage *ImageFor(NSString *url)
{
    UIImage *cached = [gImages objectForKey:url];
    if (cached) return cached;
    NSData *data = [NSData dataWithContentsOfFile:CachePath(url)];
    UIImage *image = data ? [UIImage imageWithData:data] : nil;
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

static void ShowAll(UITextView *textView)
{
    NSTextStorage *storage = textView.textStorage;
    NSMutableArray<NSValue *> *ranges = [NSMutableArray array];
    [storage enumerateAttribute:kHiddenAttribute
                        inRange:NSMakeRange(0, storage.length)
                        options:0
                     usingBlock:^(id value, NSRange range, __unused BOOL *stop) {
                         if (value) [ranges addObject:[NSValue valueWithRange:range]];
                     }];
    if (!ranges.count) return;
    [storage beginEditing];
    for (NSValue *value in ranges) {
        NSRange range = value.rangeValue;
        if (textView.font) [storage addAttribute:NSFontAttributeName value:textView.font range:range];
        if (textView.textColor) [storage addAttribute:NSForegroundColorAttributeName value:textView.textColor range:range];
        [storage removeAttribute:NSKernAttributeName range:range];
        [storage removeAttribute:kHiddenAttribute range:range];
    }
    [storage endEditing];
}

static CGRect OverlayFrame(UITextView *textView, NSUInteger location, CGFloat side)
{
    NSLayoutManager *layout = textView.layoutManager;
    NSTextContainer *container = textView.textContainer;
    [layout ensureLayoutForTextContainer:container];
    NSUInteger glyph = [layout glyphIndexForCharacterAtIndex:location];
    CGRect line = [layout lineFragmentUsedRectForGlyphAtIndex:glyph effectiveRange:NULL];
    CGRect at = [layout boundingRectForGlyphRange:NSMakeRange(glyph, 1) inTextContainer:container];
    UIEdgeInsets inset = textView.textContainerInset;
    if (line.size.height <= 0) return CGRectNull;
    return CGRectMake(at.origin.x + inset.left, line.origin.y + inset.top + (line.size.height - side) / 2, side, side);
}

static void Paint(UITextView *textView)
{
    if (!textView.window || textView.markedTextRange) return;
    RemoveOverlays(textView);
    ShowAll(textView);
    NSString *text = textView.text;
    if (!text.length || [AstralActiveEmojiPackID() isEqualToString:@"discord"]) return;
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
    if (!runs.count) return;
    NSTextStorage *storage = textView.textStorage;
    UIFont *tiny = [UIFont systemFontOfSize:kHiddenFontSize];
    [storage beginEditing];
    for (NSDictionary *run in runs) {
        NSRange range = [run[@"range"] rangeValue];
        [storage addAttributes:@{
            NSFontAttributeName : tiny,
            NSForegroundColorAttributeName : UIColor.clearColor,
            kHiddenAttribute : @YES
        }
                         range:range];
        [storage addAttribute:NSKernAttributeName value:@(side) range:range];
    }
    [storage endEditing];
    NSMutableDictionary *typing = [textView.typingAttributes mutableCopy];
    if (textView.font) typing[NSFontAttributeName] = textView.font;
    if (textView.textColor) typing[NSForegroundColorAttributeName] = textView.textColor;
    [typing removeObjectForKey:NSKernAttributeName];
    [typing removeObjectForKey:kHiddenAttribute];
    textView.typingAttributes = typing;
    for (NSDictionary *run in runs) {
        CGRect frame = OverlayFrame(textView, [run[@"range"] rangeValue].location, side);
        if (CGRectIsNull(frame)) continue;
        UIImageView *overlay = [[UIImageView alloc] initWithFrame:frame];
        overlay.image = run[@"image"];
        overlay.contentMode = UIViewContentModeScaleAspectFit;
        overlay.userInteractionEnabled = NO;
        overlay.tag = kOverlayTag;
        [textView addSubview:overlay];
    }
}

void AstralRepaintEmojiComposers(void)
{
    for (UITextView *textView in gTextViews.allObjects) Paint(textView);
}

void AstralInstallEmojiComposer(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gImages = [NSCache new];
        gLoading = [NSMutableSet set];
        gTextViews = [NSHashTable weakObjectsHashTable];
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
