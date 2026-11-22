#import "PluginSignature.h"

#import <CommonCrypto/CommonDigest.h>

#import "TomlDocument.h"

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

static NSData *SigningMessage(NSDictionary<NSString *, NSString *> *files)
{
    NSMutableString *message = [NSMutableString stringWithString:@"astral-plugin-v2\n"];
    for (NSString *path in [files.allKeys sortedArrayUsingSelector:@selector(compare:)])
        [message appendFormat:@"%@ %@\n", path, files[path]];
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

static BOOL SafePath(NSString *path)
{
    return path.length && ![path hasPrefix:@"/"] && ![path containsString:@".."] && ![path containsString:@"\\"];
}

NSDictionary<NSString *, NSString *> *AstralSignedFiles(NSData *signatureFile)
{
    NSDictionary *document = AstralParseTOML(signatureFile);
    NSDictionary *files = [document[@"files"] isKindOfClass:NSDictionary.class] ? document[@"files"] : nil;
    NSString *signature = [document[@"signature"] isKindOfClass:NSString.class] ? document[@"signature"] : nil;
    if (!files.count || !signature) return nil;
    for (NSString *path in files) {
        if (!SafePath(path) || ![files[path] isKindOfClass:NSString.class]) return nil;
    }
    return AstralVerifySignature(SigningMessage(files), signature) ? files : nil;
}

BOOL AstralVerifyPluginDirectory(NSString *directory, NSString *main)
{
    NSDictionary<NSString *, NSString *> *files =
        AstralSignedFiles([NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:@"signature"]]);
    if (!files[@"manifest.toml"] || !files[main]) return NO;
    for (NSString *path in files) {
        NSData *data = [NSData dataWithContentsOfFile:[directory stringByAppendingPathComponent:path]];
        if (!data || ![HexDigest(data) isEqualToString:files[path]]) return NO;
    }
    NSString *translations = [directory stringByAppendingPathComponent:@"i18n"];
    for (NSString *entry in [NSFileManager.defaultManager contentsOfDirectoryAtPath:translations error:nil]) {
        if (!files[[@"i18n/" stringByAppendingString:entry]]) return NO;
    }
    NSDirectoryEnumerator *walker = [NSFileManager.defaultManager enumeratorAtPath:directory];
    for (NSString *path in walker) {
        if ([path isEqualToString:@"tests"]) {
            [walker skipDescendants];
            continue;
        }
        if ([path.pathExtension isEqualToString:@"lua"] && !files[path]) return NO;
    }
    return YES;
}
