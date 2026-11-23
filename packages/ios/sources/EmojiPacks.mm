#import "EmojiPacks.h"

#include <cstring>

#import "FoundationJSON.h"
#import "AstralState.h"
#import "SharedPacks.h"

NSArray<NSDictionary *> *AstralEmojiPacks(void)
{
    return AstralObjectFromJSON(AstralSharedPacks().emoji());
}

NSString *AstralActiveEmojiPackID(void)
{
    if (!AstralSettings()) return @"discord";
    return [NSString stringWithUTF8String:AstralSharedPacks().activeEmoji().c_str()];
}

BOOL AstralEmojiAnimated(void)
{
    if (!AstralSettings()) return NO;
    return AstralSharedPacks().animated();
}

void AstralSelectEmojiPack(NSString *identifier)
{
    AstralSharedPacks().selectEmoji(identifier.UTF8String ?: "");
}

void AstralSetEmojiAnimated(BOOL animated)
{
    AstralSharedPacks().setAnimated(animated);
}

NSString *AstralEmojiURL(NSString *codepoints)
{
    if (!AstralSettings()) return nil;
    auto url = AstralSharedPacks().emojiURL(codepoints.UTF8String ?: "");
    return url.empty() ? nil : [NSString stringWithUTF8String:url.c_str()];
}

int AstralEmojiResolve(const char *codepoints, char *url, size_t capacity)
{
    if (!AstralSettings() || !codepoints || !url) return 0;
    auto resolved = AstralSharedPacks().emojiURL(codepoints);
    if (resolved.empty() || resolved.size() >= capacity) return 0;
    memcpy(url, resolved.c_str(), resolved.size() + 1);
    return 1;
}
