#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

NSArray<NSDictionary *> *_Nonnull AstralEmojiPacks(void);
NSString *_Nonnull AstralActiveEmojiPackID(void);
BOOL AstralEmojiAnimated(void);
void AstralSelectEmojiPack(NSString *_Nonnull identifier);
void AstralSetEmojiAnimated(BOOL animated);
NSString *_Nullable AstralEmojiURL(NSString *_Nonnull codepoints);
int AstralEmojiResolve(const char *_Nonnull codepoints, char *_Nonnull url, size_t capacity);

#ifdef __cplusplus
}
#endif
