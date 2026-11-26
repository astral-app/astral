#import "SvgRaster.h"

#import "AstralCore.h"

static const CGFloat kRenderScale = 3;

UIImage *AstralRenderSVG(NSString *svg, CGFloat points)
{
    if (!svg.length || points <= 0) return nil;
    AstralOwned png = astral_ios_render_svg(AstralTextOf(svg), (uint32_t)ceil(points * kRenderScale));
    if (!png.bytes) return nil;
    NSData *data = [NSData dataWithBytes:png.bytes length:png.length];
    astral_ios_free(png);
    UIImage *image = [UIImage imageWithData:data scale:kRenderScale];
    return [image imageWithRenderingMode:UIImageRenderingModeAlwaysTemplate];
}
