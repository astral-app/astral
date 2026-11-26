#import "SettingsPages.h"

#import "AstralCore.h"

static NSDictionary *Dictionary(id value)
{
    return [value isKindOfClass:NSDictionary.class] ? value : @{};
}

NSDictionary *AstralSettingsPageModel(NSString *route)
{
    return Dictionary(AstralTakeJSON(astral_ios_page_model(AstralTextOf(route))));
}

NSString *AstralSettingsPageAction(NSString *route, NSString *key)
{
    return AstralTakeString(astral_ios_page_action(AstralTextOf(route), AstralTextOf(key)));
}
