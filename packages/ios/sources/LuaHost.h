#import <Foundation/Foundation.h>

#import "PhoenixWire.h"

#ifdef __cplusplus
namespace astral {
class SettingsStore;
}
void AstralLuaHostStart(NSArray<NSString *> *directories, astral::SettingsStore *store, bool startEnabled);
extern "C" {
#endif

NSArray<NSDictionary *> *AstralLuaPlugins(void);
void AstralLuaRemoveStorage(NSString *identifier);
void AstralLuaSetEnabled(NSString *pluginID, BOOL enabled);
void AstralLuaSetSwitch(NSString *pluginID, NSString *key, BOOL on);
void AstralLuaRescan(void);
void AstralLuaRemove(NSString *pluginID);
void AstralLuaReload(NSString *pluginID);
NSString *AstralLuaEval(NSString *pluginID, NSString *code);
void AstralLuaResetSettings(NSString *pluginID);
int AstralLuaMention(const AstralMentionContext *context, AstralMentionDecoration *decoration);
size_t AstralLuaDecorate(const AstralDecorationContext *context, AstralTag *tags, size_t capacity);
int AstralLuaRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity);
void AstralLuaSetChoice(NSString *pluginID, NSString *key, NSString *value);
void AstralLuaSetNumber(NSString *pluginID, NSString *key, double value);

#ifdef __cplusplus
}
#endif
