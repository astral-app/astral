#import "DiscordCache.h"

#include "AstralDiscordCache.h"
#import "FoundationJSON.h"

static std::string Plain(NSString *value)
{
    return value.UTF8String ?: "";
}

static NSString *Native(const std::optional<std::string> &value)
{
    return value ? [NSString stringWithUTF8String:value->c_str()] : nil;
}

static NSDictionary *Dictionary(const std::optional<nlohmann::json> &value)
{
    if (!value) return nil;
    id object = AstralObjectFromJSON(*value);
    return [object isKindOfClass:NSDictionary.class] ? object : nil;
}

static void UseDiscordData(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSURL *support =
            [NSFileManager.defaultManager URLsForDirectory:NSApplicationSupportDirectory inDomains:NSUserDomainMask].firstObject;
        astral::discord::setDataDirectory(Plain(support.path));
    });
}

NSDictionary *AstralDiscordGuild(NSString *guildID)
{
    UseDiscordData();
    return Dictionary(astral::discord::guild(Plain(guildID)));
}

NSDictionary *AstralDiscordMember(NSString *guildID, NSString *userID)
{
    UseDiscordData();
    return Dictionary(astral::discord::member(Plain(guildID), Plain(userID)));
}

NSString *AstralDiscordMemberNamed(NSString *guildID, NSString *name)
{
    UseDiscordData();
    return Native(astral::discord::memberNamed(Plain(guildID), Plain(name)));
}

NSString *AstralDiscordChannelGuild(NSString *channelID)
{
    UseDiscordData();
    return Native(astral::discord::channelGuild(Plain(channelID)));
}

NSDictionary *AstralDiscordUser(NSString *userID)
{
    UseDiscordData();
    return Dictionary(astral::discord::user(Plain(userID)));
}

NSString *AstralDiscordFriendNickname(NSString *userID)
{
    UseDiscordData();
    return Native(astral::discord::friendNickname(Plain(userID)));
}

NSInteger AstralDiscordRelationship(NSString *userID)
{
    UseDiscordData();
    return astral::discord::relationshipType(Plain(userID)).value_or(0);
}

NSString *AstralDiscordMessageChannel(NSString *messageID)
{
    UseDiscordData();
    return Native(astral::discord::messageChannel(Plain(messageID)));
}

NSDictionary *AstralDiscordMessage(NSString *channelID, NSString *messageID)
{
    UseDiscordData();
    return Dictionary(astral::discord::message(Plain(channelID), Plain(messageID)));
}

NSNumber *AstralDiscordPermissions(NSString *guildID, NSString *userID, NSString *channelID)
{
    UseDiscordData();
    auto permissions = astral::discord::permissions(Plain(guildID), Plain(userID),
                                                    channelID ? std::optional<std::string>(Plain(channelID)) : std::nullopt);
    return permissions ? @(*permissions) : nil;
}
