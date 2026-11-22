#import "SoftRestart.h"

#import "AstralState.h"
#import "DevServer.h"
#import "LuaHost.h"
#import "PhoenixSettings.h"
#import "PhoenixWire.h"
#import "Themes.h"

NSDictionary *AstralSoftRestart(void)
{
    CFAbsoluteTime started = CFAbsoluteTimeGetCurrent();
    NSUInteger plugins = 0;
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        if (![plugin[@"enabled"] boolValue]) continue;
        AstralLuaReload(plugin[@"id"]);
        plugins++;
    }
    AstralApplyTheme(AstralActiveThemeID());
    int images = AstralPhoenixRefreshImages();
    AstralRefreshPhoenixSettings();
    double milliseconds = (CFAbsoluteTimeGetCurrent() - started) * 1000;
    AstralDevAppendLog(
        [NSString stringWithFormat:@"soft restart: %lu plugins, %d icons, %.0f ms", (unsigned long)plugins, images, milliseconds]);
    return @{@"plugins" : @(plugins), @"images" : @(images), @"milliseconds" : @(lround(milliseconds))};
}
