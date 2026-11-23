#import "PluginSignature.h"

#import <CommonCrypto/CommonDigest.h>

#include "AstralSignatures.h"
#import "FoundationJSON.h"

static const astral::Signatures &Verifier(void)
{
    static astral::Signatures verifier(ASTRAL_SIGNING_KEY, [](const std::string &data) {
        unsigned char digest[CC_SHA256_DIGEST_LENGTH];
        CC_SHA256(data.data(), static_cast<CC_LONG>(data.size()), digest);
        std::string hex;
        const char *digits = "0123456789abcdef";
        for (auto byte : digest) {
            hex += digits[byte >> 4];
            hex += digits[byte & 15];
        }
        return hex;
    });
    return verifier;
}

BOOL AstralVerifySignature(NSData *message, NSString *signature)
{
    if (!signature) return NO;
    return Verifier().verify(std::string_view(static_cast<const char *>(message.bytes), message.length), signature.UTF8String ?: "");
}

NSDictionary<NSString *, NSString *> *AstralSignedFiles(NSData *signatureFile)
{
    if (!signatureFile.length) return nil;
    auto files = Verifier().files(std::string(static_cast<const char *>(signatureFile.bytes), signatureFile.length));
    return files ? AstralObjectFromJSON(nlohmann::json(*files)) : nil;
}

BOOL AstralVerifyPluginDirectory(NSString *directory, NSString *main)
{
    return Verifier().directory(directory.UTF8String ?: "", main.UTF8String ?: "");
}
