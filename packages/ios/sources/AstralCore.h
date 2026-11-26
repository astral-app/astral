#import <Foundation/Foundation.h>

#include "Astral.h"

static inline AstralText AstralTextOf(NSString *_Nullable value)
{
    if (!value) return (AstralText){NULL, 0};
    const char *bytes = value.UTF8String ?: "";
    return (AstralText){(const uint8_t *)bytes, strlen(bytes)};
}

static inline AstralText AstralTextOfData(NSData *_Nullable data)
{
    if (!data) return (AstralText){NULL, 0};
    return (AstralText){data.length ? (const uint8_t *)data.bytes : (const uint8_t *)"", data.length};
}

static inline NSString *_Nonnull AstralStringOfText(AstralText text)
{
    if (!text.bytes || !text.length) return @"";
    return [[NSString alloc] initWithBytes:text.bytes length:text.length encoding:NSUTF8StringEncoding] ?: @"";
}

static inline NSData *_Nonnull AstralDataOfText(AstralText text)
{
    if (!text.bytes || !text.length) return [NSData data];
    return [NSData dataWithBytes:text.bytes length:text.length];
}

static inline NSString *_Nullable AstralTakeString(AstralOwned owned)
{
    if (!owned.bytes) return nil;
    NSString *value = [[NSString alloc] initWithBytes:owned.bytes length:owned.length encoding:NSUTF8StringEncoding] ?: @"";
    astral_ios_free(owned);
    return value;
}

static inline id _Nullable AstralTakeJSON(AstralOwned owned)
{
    if (!owned.bytes) return nil;
    NSData *data = [NSData dataWithBytesNoCopy:owned.bytes length:owned.length freeWhenDone:NO];
    id value = owned.length ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    astral_ios_free(owned);
    return value;
}

static inline NSArray<NSDictionary *> *_Nonnull AstralPlugins(void)
{
    id plugins = AstralTakeJSON(astral_ios_plugins());
    return [plugins isKindOfClass:NSArray.class] ? plugins : @[];
}

static inline void AstralDevAppendLog(NSString *_Nonnull line)
{
    astral_ios_developer_log(AstralTextOf(line));
}
