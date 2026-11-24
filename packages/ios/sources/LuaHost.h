#import <Foundation/Foundation.h>

#import "PhoenixWire.h"

#ifdef __cplusplus
namespace astral {
class SettingsStore;
}
void AstralLuaHostStart(NSArray<NSString *> *directories, astral::SettingsStore *store, bool startEnabled);
void AstralLuaHostEmitEvent(NSString *event);
extern "C" {
#endif

NSArray<NSDictionary *> *AstralLuaPlugins(void);
NSArray<NSDictionary *> *AstralLuaComposerStyles(NSString *text);
void AstralLuaRemoveStorage(NSString *identifier);
void AstralLuaSetEnabled(NSString *pluginID, BOOL enabled);
void AstralLuaSetSwitch(NSString *pluginID, NSString *key, BOOL on);
void AstralLuaRescan(void);
void AstralLuaRemove(NSString *pluginID);
void AstralLuaReload(NSString *pluginID);
NSString *AstralLuaEval(NSString *pluginID, NSString *code);
void AstralLuaResetSettings(NSString *pluginID);
int AstralLuaReaction(const AstralReactionContext *context, AstralReactionDecoration *decoration);
int AstralLuaMessageText(uint64_t messageID, char *text, size_t textCapacity, char *note, size_t noteCapacity);
int AstralLuaTyping(const AstralNameContext *context, AstralMentionDecoration *decoration);
int AstralLuaNameColor(const AstralNameContext *context, uint32_t *color);
int AstralLuaMessageTextColor(uint64_t messageID, uint64_t channelID, uint32_t *color);
int AstralLuaMessageBackground(const AstralMessageBackgroundContext *context, AstralMessageBackgroundStyle *style);
int AstralLuaPatchNode(const AstralPatchTarget *target, AstralNodePatch *patch);
int AstralLuaMention(const AstralMentionContext *context, AstralMentionDecoration *decoration);
size_t AstralLuaDecorate(const AstralDecorationContext *context, AstralTag *tags, size_t capacity);
int AstralLuaRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity);
void AstralLuaSetChoice(NSString *pluginID, NSString *key, NSString *value);
void AstralLuaSetNumber(NSString *pluginID, NSString *key, double value);

#ifdef __cplusplus
}
#endif
