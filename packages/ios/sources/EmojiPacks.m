#import "EmojiPacks.h"

#import "AstralCore.h"

NSString *AstralEmojiURL(NSString *codepoints)
{
    NSString *url = AstralTakeString(astral_ios_emoji_url(AstralTextOf(codepoints)));
    return url.length ? url : nil;
}
