#import "SvgRaster.h"

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

typedef struct {
    const char *text;
    size_t length;
    size_t offset;
} Scanner;

static void SkipSeparators(Scanner *scanner)
{
    while (scanner->offset < scanner->length) {
        char character = scanner->text[scanner->offset];
        if (character == ' ' || character == ',' || character == '\n' || character == '\t' || character == '\r')
            scanner->offset++;
        else
            break;
    }
}

static BOOL NextNumber(Scanner *scanner, CGFloat *value)
{
    SkipSeparators(scanner);
    if (scanner->offset >= scanner->length) return NO;
    const char *start = scanner->text + scanner->offset;
    char *end = NULL;
    double parsed = strtod(start, &end);
    if (end == start) return NO;
    scanner->offset += (size_t)(end - start);
    *value = parsed;
    return YES;
}

static BOOL NextFlag(Scanner *scanner, CGFloat *value)
{
    SkipSeparators(scanner);
    if (scanner->offset >= scanner->length) return NO;
    char character = scanner->text[scanner->offset];
    if (character != '0' && character != '1') return NO;
    scanner->offset++;
    *value = character == '1';
    return YES;
}

static void AddArc(CGMutablePathRef path, CGPoint from, CGFloat rx, CGFloat ry, CGFloat rotation, BOOL large, BOOL sweep, CGPoint to)
{
    if (rx == 0 || ry == 0) {
        CGPathAddLineToPoint(path, NULL, to.x, to.y);
        return;
    }
    rx = fabs(rx);
    ry = fabs(ry);
    CGFloat phi = rotation * M_PI / 180, cosPhi = cos(phi), sinPhi = sin(phi);
    CGFloat dx = (from.x - to.x) / 2, dy = (from.y - to.y) / 2;
    CGFloat x1 = cosPhi * dx + sinPhi * dy, y1 = -sinPhi * dx + cosPhi * dy;
    CGFloat lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if (lambda > 1) {
        rx *= sqrt(lambda);
        ry *= sqrt(lambda);
    }
    CGFloat numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    CGFloat denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    CGFloat factor = denominator == 0 ? 0 : sqrt(fmax(0, numerator / denominator)) * (large == sweep ? -1 : 1);
    CGFloat cx1 = factor * rx * y1 / ry, cy1 = -factor * ry * x1 / rx;
    CGFloat cx = cosPhi * cx1 - sinPhi * cy1 + (from.x + to.x) / 2;
    CGFloat cy = sinPhi * cx1 + cosPhi * cy1 + (from.y + to.y) / 2;
    CGFloat start = atan2((y1 - cy1) / ry, (x1 - cx1) / rx);
    CGFloat end = atan2((-y1 - cy1) / ry, (-x1 - cx1) / rx);
    CGFloat delta = end - start;
    if (sweep && delta < 0) delta += 2 * M_PI;
    if (!sweep && delta > 0) delta -= 2 * M_PI;
    CGAffineTransform transform = CGAffineTransformMakeTranslation(cx, cy);
    transform = CGAffineTransformRotate(transform, phi);
    transform = CGAffineTransformScale(transform, rx, ry);
    CGPathAddRelativeArc(path, &transform, 0, 0, 1, start, delta);
}

static CGPathRef PathFromData(NSString *data)
{
    CGMutablePathRef path = CGPathCreateMutable();
    const char *text = data.UTF8String;
    Scanner scanner = {text, strlen(text), 0};
    CGPoint current = CGPointZero, start = CGPointZero, control = CGPointZero;
    char command = 0, previous = 0;
    while (YES) {
        SkipSeparators(&scanner);
        if (scanner.offset >= scanner.length) break;
        char character = text[scanner.offset];
        if ((character >= 'A' && character <= 'Z') || (character >= 'a' && character <= 'z')) {
            command = character;
            scanner.offset++;
        } else if (!command) {
            break;
        }
        BOOL relative = command >= 'a';
        char upper = (char)(relative ? command - 32 : command);
        CGFloat a, b, c, d, e, f, g;
        size_t before = scanner.offset;
        switch (upper) {
        case 'Z':
            CGPathCloseSubpath(path);
            current = start;
            previous = upper;
            command = 0;
            continue;
        case 'M':
            if (!NextNumber(&scanner, &a) || !NextNumber(&scanner, &b)) goto done;
            current = relative ? CGPointMake(current.x + a, current.y + b) : CGPointMake(a, b);
            CGPathMoveToPoint(path, NULL, current.x, current.y);
            start = current;
            command = relative ? 'l' : 'L';
            break;
        case 'L':
            if (!NextNumber(&scanner, &a) || !NextNumber(&scanner, &b)) goto done;
            current = relative ? CGPointMake(current.x + a, current.y + b) : CGPointMake(a, b);
            CGPathAddLineToPoint(path, NULL, current.x, current.y);
            break;
        case 'H':
            if (!NextNumber(&scanner, &a)) goto done;
            current.x = relative ? current.x + a : a;
            CGPathAddLineToPoint(path, NULL, current.x, current.y);
            break;
        case 'V':
            if (!NextNumber(&scanner, &a)) goto done;
            current.y = relative ? current.y + a : a;
            CGPathAddLineToPoint(path, NULL, current.x, current.y);
            break;
        case 'C': {
            if (!NextNumber(&scanner, &a) || !NextNumber(&scanner, &b) || !NextNumber(&scanner, &c) || !NextNumber(&scanner, &d) ||
                !NextNumber(&scanner, &e) || !NextNumber(&scanner, &f))
                goto done;
            CGPoint origin = relative ? current : CGPointZero;
            CGPoint first = CGPointMake(origin.x + a, origin.y + b);
            control = CGPointMake(origin.x + c, origin.y + d);
            current = CGPointMake(origin.x + e, origin.y + f);
            CGPathAddCurveToPoint(path, NULL, first.x, first.y, control.x, control.y, current.x, current.y);
            previous = 'C';
            continue;
        }
        case 'S': {
            if (!NextNumber(&scanner, &c) || !NextNumber(&scanner, &d) || !NextNumber(&scanner, &e) || !NextNumber(&scanner, &f)) goto done;
            CGPoint origin = relative ? current : CGPointZero;
            CGPoint first =
                (previous == 'C' || previous == 'S') ? CGPointMake(2 * current.x - control.x, 2 * current.y - control.y) : current;
            control = CGPointMake(origin.x + c, origin.y + d);
            current = CGPointMake(origin.x + e, origin.y + f);
            CGPathAddCurveToPoint(path, NULL, first.x, first.y, control.x, control.y, current.x, current.y);
            previous = 'S';
            continue;
        }
        case 'Q': {
            if (!NextNumber(&scanner, &a) || !NextNumber(&scanner, &b) || !NextNumber(&scanner, &e) || !NextNumber(&scanner, &f)) goto done;
            CGPoint origin = relative ? current : CGPointZero;
            control = CGPointMake(origin.x + a, origin.y + b);
            current = CGPointMake(origin.x + e, origin.y + f);
            CGPathAddQuadCurveToPoint(path, NULL, control.x, control.y, current.x, current.y);
            previous = 'Q';
            continue;
        }
        case 'T': {
            if (!NextNumber(&scanner, &e) || !NextNumber(&scanner, &f)) goto done;
            CGPoint origin = relative ? current : CGPointZero;
            control = (previous == 'Q' || previous == 'T') ? CGPointMake(2 * current.x - control.x, 2 * current.y - control.y) : current;
            current = CGPointMake(origin.x + e, origin.y + f);
            CGPathAddQuadCurveToPoint(path, NULL, control.x, control.y, current.x, current.y);
            previous = 'T';
            continue;
        }
        case 'A': {
            if (!NextNumber(&scanner, &a) || !NextNumber(&scanner, &b) || !NextNumber(&scanner, &c) || !NextFlag(&scanner, &d) ||
                !NextFlag(&scanner, &e) || !NextNumber(&scanner, &f) || !NextNumber(&scanner, &g))
                goto done;
            CGPoint target = relative ? CGPointMake(current.x + f, current.y + g) : CGPointMake(f, g);
            AddArc(path, current, a, b, c, d != 0, e != 0, target);
            current = target;
            break;
        }
        default:
            goto done;
        }
        previous = upper;
        if (scanner.offset == before) break;
    }
done:
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
        const char *text = attributes[@"points"].UTF8String ?: "";
        Scanner scanner = {text, strlen(text), 0};
        CGFloat x, y;
        BOOL first = YES;
        while (NextNumber(&scanner, &x) && NextNumber(&scanner, &y)) {
            if (first)
                CGPathMoveToPoint(path, NULL, x, y);
            else
                CGPathAddLineToPoint(path, NULL, x, y);
            first = NO;
        }
        if ([name isEqualToString:@"polygon"]) CGPathCloseSubpath(path);
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
