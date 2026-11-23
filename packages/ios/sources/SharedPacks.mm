#import "SharedPacks.h"

#import "AstralState.h"

astral::Packs &AstralSharedPacks(void)
{
    static astral::Packs packs(*AstralSettings(), AstralStateDirectory().UTF8String, AstralResourceDirectory().UTF8String);
    return packs;
}

astral::Themes &AstralSharedThemes(void)
{
    static astral::Themes themes(*AstralSettings(), AstralStateDirectory().UTF8String, AstralResourceDirectory().UTF8String);
    return themes;
}
