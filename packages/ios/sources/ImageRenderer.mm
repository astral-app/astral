#import "ImageRenderer.h"

#import <UIKit/UIKit.h>

static UIColor *ColorOf(id value, UIColor *fallback)
{
    if (![value isKindOfClass:NSNumber.class]) return fallback;
    uint32_t argb = (uint32_t)[value unsignedLongLongValue];
    return [UIColor colorWithRed:((argb >> 16) & 0xFF) / 255.0
                           green:((argb >> 8) & 0xFF) / 255.0
                            blue:(argb & 0xFF) / 255.0
                           alpha:((argb >> 24) & 0xFF) / 255.0];
}

static CGFloat Number(NSDictionary *item, NSString *key, CGFloat fallback)
{
    id value = item[key];
    return [value isKindOfClass:NSNumber.class] ? [value doubleValue] : fallback;
}

static UIFont *FontOf(NSDictionary *item)
{
    CGFloat size = Number(item, @"size", 16);
    BOOL bold = [item[@"weight"] isEqual:@"bold"];
    BOOL semibold = [item[@"weight"] isEqual:@"semibold"];
    UIFontWeight weight = bold ? UIFontWeightBold : semibold ? UIFontWeightSemibold : UIFontWeightRegular;
    if ([item[@"mono"] boolValue]) return [UIFont monospacedSystemFontOfSize:size weight:weight];
    return [UIFont systemFontOfSize:size weight:weight];
}

static void DrawText(NSDictionary *item)
{
    NSString *text = [item[@"text"] isKindOfClass:NSString.class] ? item[@"text"] : @"";
    NSMutableParagraphStyle *paragraph = [NSMutableParagraphStyle new];
    paragraph.lineBreakMode = NSLineBreakByTruncatingTail;
    if ([item[@"align"] isEqual:@"right"]) paragraph.alignment = NSTextAlignmentRight;
    if ([item[@"align"] isEqual:@"center"]) paragraph.alignment = NSTextAlignmentCenter;
    NSDictionary *attributes = @{
        NSFontAttributeName : FontOf(item),
        NSForegroundColorAttributeName : ColorOf(item[@"color"], UIColor.whiteColor),
        NSParagraphStyleAttributeName : paragraph
    };
    CGFloat width = Number(item, @"width", 4096);
    CGFloat lineHeight = FontOf(item).lineHeight;
    CGRect frame = CGRectMake(Number(item, @"x", 0), Number(item, @"y", 0), width, Number(item, @"height", lineHeight));
    [text drawWithRect:frame
               options:NSStringDrawingUsesLineFragmentOrigin | NSStringDrawingTruncatesLastVisibleLine
            attributes:attributes
               context:nil];
}

NSData *AstralRenderImage(NSDictionary *spec)
{
    CGFloat width = [spec[@"width"] doubleValue], height = [spec[@"height"] doubleValue];
    CGFloat scale = spec[@"scale"] ? [spec[@"scale"] doubleValue] : 2;
    NSArray *items = [spec[@"items"] isKindOfClass:NSArray.class] ? spec[@"items"] : @[];
    UIGraphicsImageRendererFormat *format = [UIGraphicsImageRendererFormat preferredFormat];
    format.scale = scale;
    format.opaque = NO;
    UIGraphicsImageRenderer *renderer = [[UIGraphicsImageRenderer alloc] initWithSize:CGSizeMake(width, height) format:format];
    return [renderer PNGDataWithActions:^(UIGraphicsImageRendererContext *context) {
        if (spec[@"background"]) {
            [ColorOf(spec[@"background"], UIColor.clearColor) setFill];
            UIRectFill(CGRectMake(0, 0, width, height));
        }
        for (NSDictionary *item in items) {
            if (![item isKindOfClass:NSDictionary.class]) continue;
            NSString *type = item[@"type"];
            if ([type isEqual:@"rect"]) {
                CGRect frame = CGRectMake(Number(item, @"x", 0), Number(item, @"y", 0), Number(item, @"w", 0), Number(item, @"h", 0));
                UIBezierPath *path = [UIBezierPath bezierPathWithRoundedRect:frame cornerRadius:Number(item, @"radius", 0)];
                [ColorOf(item[@"color"], UIColor.clearColor) setFill];
                [path fill];
                if (item[@"border"]) {
                    [ColorOf(item[@"border"], UIColor.clearColor) setStroke];
                    path.lineWidth = Number(item, @"border_width", 1);
                    [path stroke];
                }
            } else if ([type isEqual:@"circle"]) {
                CGFloat radius = Number(item, @"r", 4);
                UIBezierPath *path =
                    [UIBezierPath bezierPathWithOvalInRect:CGRectMake(Number(item, @"x", 0) - radius, Number(item, @"y", 0) - radius,
                                                                      radius * 2, radius * 2)];
                [ColorOf(item[@"color"], UIColor.whiteColor) setFill];
                [path fill];
            } else if ([type isEqual:@"text"]) {
                DrawText(item);
            }
        }
    }];
}
