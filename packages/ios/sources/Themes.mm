#import "Themes.h"

#import "FoundationJSON.h"
#import "PhoenixSettings.h"
#import "SharedPacks.h"

NSString *AstralLocalThemesDirectory(void)
{
    return [NSString stringWithUTF8String:AstralSharedThemes().directory().c_str()];
}

NSArray<NSDictionary *> *AstralThemes(void)
{
    return AstralObjectFromJSON(AstralSharedThemes().list());
}

NSString *AstralActiveThemeID(void)
{
    return [NSString stringWithUTF8String:AstralSharedThemes().active().c_str()];
}

void AstralApplyTheme(NSString *identifier)
{
    auto style = AstralSharedThemes().select(identifier.UTF8String ?: "discord");
    AstralPhoenixUpdateTheme(^{
        style.apply();
    });
}

NSString *AstralImportUnboundTheme(NSData *data, NSString *fallbackName)
{
    if (!data.length) return @"not an Unbound theme";
    std::string source(static_cast<const char *>(data.bytes), data.length);
    auto error = AstralSharedThemes().importUnbound(source, fallbackName.UTF8String ?: "Imported");
    if (error) return [NSString stringWithUTF8String:error->c_str()];
    AstralApplyTheme(AstralActiveThemeID());
    return nil;
}
