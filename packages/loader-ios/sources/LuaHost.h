#import <Foundation/Foundation.h>

#import "PhoenixWire.h"

#ifdef __cplusplus
namespace loader {
class SettingsStore;
}
void AstralLuaHostStart(NSArray<NSString *> *directories, loader::SettingsStore *store, bool startEnabled);
extern "C" {
#endif

NSArray<NSDictionary *> *AstralLuaPlugins(void);
void AstralLuaRemoveStorage(NSString *identifier);
void AstralLuaSetEnabled(NSString *pluginID, BOOL enabled);
void AstralLuaSetSwitch(NSString *pluginID, NSString *key, BOOL on);
void AstralLuaRescan(void);
void AstralLuaRemove(NSString *pluginID);
NSString *AstralLuaEval(NSString *pluginID, NSString *code);
void AstralLuaResetSettings(NSString *pluginID);
size_t AstralLuaDecorate(const AstralDecorationContext *context, AstralTag *tags, size_t capacity);
int AstralLuaRelabel(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity);
void AstralLuaSetChoice(NSString *pluginID, NSString *key, NSString *value);

#ifdef __cplusplus
}
#endif
