#import "SvgRaster.h"

#import "AstralCore.h"

typedef struct {
    BOOL fill;
    BOOL stroke;
    CGFloat width;
    CGLineCap cap;
    CGLineJoin join;
} Paint;

@interface AstralSVGParser : NSObject <NSXMLParserDelegate>
@property (nonatomic) CGRect viewBox;
@property (nonatomic) CGContextRef context;
@property (nonatomic, strong) NSMutableArray<NSValue *> *paints;
@end

static BOOL Painted(NSString *value, BOOL fallback)
{
    if (!value) return fallback;
    return ![value isEqualToString:@"none"] && ![value isEqualToString:@"transparent"];
}

static CGLineCap Cap(NSString *value, CGLineCap fallback)
{
    if ([value isEqualToString:@"round"]) return kCGLineCapRound;
    if ([value isEqualToString:@"square"]) return kCGLineCapSquare;
    if ([value isEqualToString:@"butt"]) return kCGLineCapButt;
    return fallback;
}

static CGLineJoin Join(NSString *value, CGLineJoin fallback)
{
    if ([value isEqualToString:@"round"]) return kCGLineJoinRound;
    if ([value isEqualToString:@"bevel"]) return kCGLineJoinBevel;
    if ([value isEqualToString:@"miter"]) return kCGLineJoinMiter;
    return fallback;
}

static Paint Inherit(Paint parent, NSDictionary<NSString *, NSString *> *attributes)
{
    Paint paint = parent;
    paint.fill = Painted(attributes[@"fill"], parent.fill);
    paint.stroke = Painted(attributes[@"stroke"], parent.stroke);
    if (attributes[@"stroke-width"]) paint.width = attributes[@"stroke-width"].doubleValue;
    paint.cap = Cap(attributes[@"stroke-linecap"], parent.cap);
    paint.join = Join(attributes[@"stroke-linejoin"], parent.join);
    return paint;
}

static void AddSegment(CGMutablePathRef path, const AstralPathSegment *segment)
{
    const double *v = segment->values;
    switch (segment->kind) {
    case AstralSegmentKind_Move:
        CGPathMoveToPoint(path, NULL, v[0], v[1]);
        break;
    case AstralSegmentKind_Line:
        CGPathAddLineToPoint(path, NULL, v[0], v[1]);
        break;
    case AstralSegmentKind_Cubic:
        CGPathAddCurveToPoint(path, NULL, v[0], v[1], v[2], v[3], v[4], v[5]);
        break;
    case AstralSegmentKind_Quad:
        CGPathAddQuadCurveToPoint(path, NULL, v[0], v[1], v[2], v[3]);
        break;
    case AstralSegmentKind_Arc: {
        CGAffineTransform transform = CGAffineTransformMakeTranslation(v[0], v[1]);
        transform = CGAffineTransformRotate(transform, v[4]);
        transform = CGAffineTransformScale(transform, v[2], v[3]);
        CGPathAddRelativeArc(path, &transform, 0, 0, 1, v[5], v[6]);
        break;
    }
    case AstralSegmentKind_Close:
        CGPathCloseSubpath(path);
        break;
    }
}

static CGPathRef PathFromData(NSString *data)
{
    CGMutablePathRef path = CGPathCreateMutable();
    AstralPath parsed = astral_ios_svg_path(AstralTextOf(data));
    for (size_t index = 0; index < parsed.count; index++) AddSegment(path, &parsed.segments[index]);
    astral_ios_free_path(parsed);
    return path;
}

static CGFloat Number(NSDictionary<NSString *, NSString *> *attributes, NSString *key)
{
    return attributes[key].doubleValue;
}

static CGPathRef ShapePath(NSString *name, NSDictionary<NSString *, NSString *> *attributes)
{
    if ([name isEqualToString:@"path"]) return attributes[@"d"] ? PathFromData(attributes[@"d"]) : NULL;
    CGMutablePathRef path = CGPathCreateMutable();
    if ([name isEqualToString:@"line"]) {
        CGPathMoveToPoint(path, NULL, Number(attributes, @"x1"), Number(attributes, @"y1"));
        CGPathAddLineToPoint(path, NULL, Number(attributes, @"x2"), Number(attributes, @"y2"));
    } else if ([name isEqualToString:@"rect"]) {
        CGRect rect =
            CGRectMake(Number(attributes, @"x"), Number(attributes, @"y"), Number(attributes, @"width"), Number(attributes, @"height"));
        CGFloat rx = attributes[@"rx"] ? Number(attributes, @"rx") : Number(attributes, @"ry");
        CGFloat ry = attributes[@"ry"] ? Number(attributes, @"ry") : rx;
        rx = fmin(rx, rect.size.width / 2);
        ry = fmin(ry, rect.size.height / 2);
        CGPathAddRoundedRect(path, NULL, rect, rx, ry);
    } else if ([name isEqualToString:@"circle"]) {
        CGFloat r = Number(attributes, @"r");
        CGPathAddEllipseInRect(path, NULL, CGRectMake(Number(attributes, @"cx") - r, Number(attributes, @"cy") - r, 2 * r, 2 * r));
    } else if ([name isEqualToString:@"ellipse"]) {
        CGFloat rx = Number(attributes, @"rx"), ry = Number(attributes, @"ry");
        CGPathAddEllipseInRect(path, NULL, CGRectMake(Number(attributes, @"cx") - rx, Number(attributes, @"cy") - ry, 2 * rx, 2 * ry));
    } else if ([name isEqualToString:@"polyline"] || [name isEqualToString:@"polygon"]) {
        CGPathRelease(path);
        NSString *closing = [name isEqualToString:@"polygon"] ? @"z" : @"";
        return PathFromData([NSString stringWithFormat:@"M%@%@", attributes[@"points"] ?: @"", closing]);
    } else {
        CGPathRelease(path);
        return NULL;
    }
    return path;
}

@implementation AstralSVGParser

- (void)parser:(NSXMLParser *)parser
    didStartElement:(NSString *)name
       namespaceURI:(NSString *)namespace
      qualifiedName:(NSString *)qualified
         attributes:(NSDictionary<NSString *, NSString *> *)attributes
{
    Paint parent = {YES, NO, 1, kCGLineCapButt, kCGLineJoinMiter};
    if (self.paints.count) parent = *(Paint *)self.paints.lastObject.pointerValue;
    Paint paint = Inherit(parent, attributes);
    Paint *stored = malloc(sizeof(Paint));
    *stored = paint;
    [self.paints addObject:[NSValue valueWithPointer:stored]];
    if ([name isEqualToString:@"svg"]) {
        NSArray<NSString *> *parts =
            [attributes[@"viewBox"] componentsSeparatedByCharactersInSet:[NSCharacterSet characterSetWithCharactersInString:@" ,"]];
        NSMutableArray<NSNumber *> *numbers = [NSMutableArray array];
        for (NSString *part in parts)
            if (part.length) [numbers addObject:@(part.doubleValue)];
        if (numbers.count == 4)
            self.viewBox = CGRectMake(numbers[0].doubleValue, numbers[1].doubleValue, numbers[2].doubleValue, numbers[3].doubleValue);
        return;
    }
    CGPathRef path = ShapePath(name, attributes);
    if (!path) return;
    CGContextRef context = self.context;
    if (paint.fill) {
        CGContextAddPath(context, path);
        CGContextFillPath(context);
    }
    if (paint.stroke && paint.width > 0) {
        CGContextSetLineWidth(context, paint.width);
        CGContextSetLineCap(context, paint.cap);
        CGContextSetLineJoin(context, paint.join);
        CGContextAddPath(context, path);
        CGContextStrokePath(context);
    }
    CGPathRelease(path);
}

- (void)parser:(NSXMLParser *)parser didEndElement:(NSString *)name namespaceURI:(NSString *)namespace qualifiedName:(NSString *)qualified
{
    NSValue *last = self.paints.lastObject;
    if (!last) return;
    free(last.pointerValue);
    [self.paints removeLastObject];
}

@end

static CGRect ViewBox(NSString *svg)
{
    NSRegularExpression *expression = [NSRegularExpression regularExpressionWithPattern:@"viewBox=\"([^\"]+)\"" options:0 error:nil];
    NSTextCheckingResult *match = [expression firstMatchInString:svg options:0 range:NSMakeRange(0, svg.length)];
    if (!match) return CGRectMake(0, 0, 24, 24);
    NSArray<NSString *> *parts = [[svg substringWithRange:[match rangeAtIndex:1]]
        componentsSeparatedByCharactersInSet:[NSCharacterSet characterSetWithCharactersInString:@" ,"]];
    NSMutableArray<NSNumber *> *numbers = [NSMutableArray array];
    for (NSString *part in parts)
        if (part.length) [numbers addObject:@(part.doubleValue)];
    return numbers.count == 4 ? CGRectMake(numbers[0].doubleValue, numbers[1].doubleValue, numbers[2].doubleValue, numbers[3].doubleValue)
                              : CGRectMake(0, 0, 24, 24);
}

UIImage *AstralRenderSVG(NSString *svg, CGFloat points)
{
    NSData *data = [svg dataUsingEncoding:NSUTF8StringEncoding];
    if (!data.length || points <= 0) return nil;
    CGRect viewBox = ViewBox(svg);
    if (viewBox.size.width <= 0 || viewBox.size.height <= 0) return nil;
    UIGraphicsImageRendererFormat *format = [UIGraphicsImageRendererFormat preferredFormat];
    format.scale = 3;
    format.opaque = NO;
    UIGraphicsImageRenderer *renderer = [[UIGraphicsImageRenderer alloc] initWithSize:CGSizeMake(points, points) format:format];
    UIImage *image = [renderer imageWithActions:^(UIGraphicsImageRendererContext *rendererContext) {
        CGContextRef context = rendererContext.CGContext;
        CGFloat scale = fmin(points / viewBox.size.width, points / viewBox.size.height);
        CGContextTranslateCTM(context, (points - viewBox.size.width * scale) / 2, (points - viewBox.size.height * scale) / 2);
        CGContextScaleCTM(context, scale, scale);
        CGContextTranslateCTM(context, -viewBox.origin.x, -viewBox.origin.y);
        CGContextSetFillColorWithColor(context, UIColor.whiteColor.CGColor);
        CGContextSetStrokeColorWithColor(context, UIColor.whiteColor.CGColor);
        AstralSVGParser *delegate = [AstralSVGParser new];
        delegate.context = context;
        delegate.paints = [NSMutableArray array];
        NSXMLParser *parser = [[NSXMLParser alloc] initWithData:data];
        parser.delegate = delegate;
        [parser parse];
        for (NSValue *value in delegate.paints) free(value.pointerValue);
    }];
    return [image imageWithRenderingMode:UIImageRenderingModeAlwaysTemplate];
}
