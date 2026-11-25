#import "Backup.h"

#import <UIKit/UIKit.h>

#include "AstralBackup.h"
#include "AstralSettings.h"
#import "AstralState.h"
#import "FilePicker.h"
#import "FoundationJSON.h"
#import "LuaHost.h"
#import "PhoenixImage.h"
#import "Themes.h"

NSString *AstralBackupTOML(void)
{
    std::string text = astral::backup::exportTOML(*AstralSettings(), AstralVersion().UTF8String, AstralJSONFromObject(AstralLuaPlugins()),
                                                  AstralJSONFromObject(AstralThemes()));
    return [NSString stringWithUTF8String:text.c_str()];
}

NSString *AstralApplyBackup(NSData *data, void (^changed)(void))
{
    void (^notify)(void) = [changed copy];
    std::string contents(static_cast<const char *>(data.bytes), data.length);
    auto failure =
        astral::backup::import(contents, *AstralSettings(), AstralJSONFromObject(AstralLuaPlugins()), AstralJSONFromObject(AstralThemes()),
                               [notify] { dispatch_async(dispatch_get_main_queue(), notify); });
    if (failure) return [NSString stringWithUTF8String:failure->c_str()];
    AstralApplyTheme(AstralActiveThemeID());
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        BOOL enabled = AstralSettings()->getBoolean("plugins", [[plugin[@"id"] stringByAppendingString:@".enabled"] UTF8String],
                                                    [plugin[@"enabled"] boolValue]);
        AstralLuaSetEnabled(plugin[@"id"], enabled);
    }
    notify();
    return nil;
}

static UIViewController *TopController(void)
{
    UIViewController *controller = AstralPhoenixWindow().rootViewController;
    while (controller.presentedViewController) controller = controller.presentedViewController;
    return controller;
}

void AstralExportBackup(void)
{
    NSString *text = AstralBackupTOML();
    NSString *path = [NSTemporaryDirectory() stringByAppendingPathComponent:@"astral-backup.toml"];
    if (![text writeToFile:path atomically:YES encoding:NSUTF8StringEncoding error:nil]) return;
    UIActivityViewController *sheet = [[UIActivityViewController alloc] initWithActivityItems:@[ [NSURL fileURLWithPath:path] ]
                                                                        applicationActivities:nil];
    UIViewController *top = TopController();
    sheet.popoverPresentationController.sourceView = top.view;
    [top presentViewController:sheet animated:YES completion:nil];
}

void AstralImportBackup(void (^changed)(void))
{
    AstralPickFile(@[ @"toml" ], ^(NSData *data, NSString *name) {
        NSString *failure = data ? AstralApplyBackup(data, changed) : @"could not read the file";
        AstralShowAlert(failure ? @"Import failed" : @"Settings imported", failure ?: @"Missing marketplace addons are being installed.");
    });
}
