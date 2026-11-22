#import "PluginSignature.h"

#import <CommonCrypto/CommonDigest.h>

extern "C" {
#include "monocypher-ed25519.h"
}

static NSString *HexDigest(NSData *data)
{
    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    CC_SHA256(data.bytes, (CC_LONG)data.length, digest);
    NSMutableString *hex = [NSMutableString stringWithCapacity:CC_SHA256_DIGEST_LENGTH * 2];
    for (int index = 0; index < CC_SHA256_DIGEST_LENGTH; index++) [hex appendFormat:@"%02x", digest[index]];
    return hex;
}

NSData *AstralPluginSigningMessage(NSData *manifest, NSData *source)
{
    NSString *message = [NSString stringWithFormat:@"astral-plugin-v1\n%@\n%@\n", HexDigest(manifest), HexDigest(source)];
    return [message dataUsingEncoding:NSUTF8StringEncoding];
}

BOOL AstralVerifySignature(NSData *message, NSString *signature)
{
    NSData *key = [[NSData alloc] initWithBase64EncodedString:@ASTRAL_SIGNING_KEY options:0];
    NSString *trimmed = [signature stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    NSData *bytes = trimmed ? [[NSData alloc] initWithBase64EncodedString:trimmed options:0] : nil;
    if (key.length != 32 || bytes.length != 64) return NO;
    return crypto_ed25519_check((const uint8_t *)bytes.bytes, (const uint8_t *)key.bytes, (const uint8_t *)message.bytes, message.length) ==
           0;
}
