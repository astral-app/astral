#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSDictionary *_Nullable AstralDiscordGuild(NSString *_Nonnull guildID);
NSDictionary *_Nullable AstralDiscordMember(NSString *_Nonnull guildID, NSString *_Nonnull userID);
NSString *_Nullable AstralDiscordMemberNamed(NSString *_Nonnull guildID, NSString *_Nonnull name);
NSString *_Nullable AstralDiscordChannelGuild(NSString *_Nonnull channelID);
NSDictionary *_Nullable AstralDiscordUser(NSString *_Nonnull userID);
NSString *_Nullable AstralDiscordFriendNickname(NSString *_Nonnull userID);
NSInteger AstralDiscordRelationship(NSString *_Nonnull userID);
NSString *_Nullable AstralDiscordMessageChannel(NSString *_Nonnull messageID);
NSDictionary *_Nullable AstralDiscordMessage(NSString *_Nonnull channelID, NSString *_Nonnull messageID);
NSNumber *_Nullable AstralDiscordPermissions(NSString *_Nonnull guildID, NSString *_Nonnull userID, NSString *_Nullable channelID);

#ifdef __cplusplus
}
#endif
