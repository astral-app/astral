#import "DiscordSession.h"

#import <Security/Security.h>

static NSString *const kStorageService = @"com.discord.libdiscord.storage";

static NSRegularExpression *TokenShape(void)
{
    static NSRegularExpression *expression;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        expression = [NSRegularExpression regularExpressionWithPattern:@"^[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{4,}\\.[A-Za-z0-9_-]{20,}$"
                                                               options:0
                                                                 error:nil];
    });
    return expression;
}

static NSString *TokenFromData(NSData *data)
{
    NSString *text = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    text = [text stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    if ([text hasPrefix:@"\""] && [text hasSuffix:@"\""] && text.length > 2)
        text = [text substringWithRange:NSMakeRange(1, text.length - 2)];
    if (text && [TokenShape() firstMatchInString:text options:0 range:NSMakeRange(0, text.length)]) return text;
    id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    NSString *field = [parsed isKindOfClass:NSDictionary.class] ? parsed[@"token"] : nil;
    if ([field isKindOfClass:NSString.class] && [TokenShape() firstMatchInString:field options:0 range:NSMakeRange(0, field.length)])
        return field;
    return nil;
}

static NSArray<NSString *> *TokenAccounts(void)
{
    NSDictionary *query = @{
        (__bridge id)kSecClass : (__bridge id)kSecClassGenericPassword,
        (__bridge id)kSecAttrService : kStorageService,
        (__bridge id)kSecReturnAttributes : @YES,
        (__bridge id)kSecMatchLimit : (__bridge id)kSecMatchLimitAll
    };
    CFTypeRef result = NULL;
    if (SecItemCopyMatching((__bridge CFDictionaryRef)query, &result) != errSecSuccess) return @[];
    NSMutableArray *accounts = [NSMutableArray array];
    for (NSDictionary *item in (__bridge_transfer NSArray *)result) {
        NSString *account = [item[(__bridge id)kSecAttrAccount] description];
        if ([account hasSuffix:@":token"] && ![account hasPrefix:@"multi:"]) [accounts addObject:account];
    }
    return accounts;
}

static NSData *ItemData(NSString *account)
{
    NSDictionary *query = @{
        (__bridge id)kSecClass : (__bridge id)kSecClassGenericPassword,
        (__bridge id)kSecAttrService : kStorageService,
        (__bridge id)kSecAttrAccount : account,
        (__bridge id)kSecReturnData : @YES,
        (__bridge id)kSecMatchLimit : (__bridge id)kSecMatchLimitOne
    };
    CFTypeRef result = NULL;
    if (SecItemCopyMatching((__bridge CFDictionaryRef)query, &result) != errSecSuccess) return nil;
    return (__bridge_transfer NSData *)result;
}

NSString *AstralDiscordSessionToken(void)
{
    for (NSString *account in TokenAccounts()) {
        NSString *token = TokenFromData(ItemData(account));
        if (token) return token;
    }
    return nil;
}
