#import "SyncFolder.h"

#import <UIKit/UIKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

#import "AstralCore.h"
#import "PhoenixImage.h"

static NSString *const kBookmarkKey = @"astral.sync.folder";
static NSString *const kFileName = @"astral-sync.toml";

@interface AstralSyncPicker : NSObject <UIDocumentPickerDelegate>
@end

static AstralSyncPicker *gPicker;

static NSURL *ResolvedFolder(void)
{
    NSData *bookmark = [NSUserDefaults.standardUserDefaults dataForKey:kBookmarkKey];
    if (!bookmark) return nil;
    BOOL stale = NO;
    NSURL *folder = [NSURL URLByResolvingBookmarkData:bookmark options:0 relativeToURL:nil bookmarkDataIsStale:&stale error:nil];
    if (folder && stale) {
        NSData *fresh = [folder bookmarkDataWithOptions:0 includingResourceValuesForKeys:nil relativeToURL:nil error:nil];
        if (fresh) [NSUserDefaults.standardUserDefaults setObject:fresh forKey:kBookmarkKey];
    }
    return folder;
}

static id WithFolder(id (^work)(NSURL *file))
{
    NSURL *folder = ResolvedFolder();
    if (!folder) return nil;
    BOOL scoped = [folder startAccessingSecurityScopedResource];
    id result = work([folder URLByAppendingPathComponent:kFileName]);
    if (scoped) [folder stopAccessingSecurityScopedResource];
    return result;
}

@implementation AstralSyncPicker
- (void)documentPicker:(UIDocumentPickerViewController *)controller didPickDocumentsAtURLs:(NSArray<NSURL *> *)urls
{
    NSURL *folder = urls.firstObject;
    BOOL scoped = [folder startAccessingSecurityScopedResource];
    NSData *bookmark = [folder bookmarkDataWithOptions:0 includingResourceValuesForKeys:nil relativeToURL:nil error:nil];
    if (scoped) [folder stopAccessingSecurityScopedResource];
    gPicker = nil;
    if (!bookmark) return;
    [NSUserDefaults.standardUserDefaults setObject:bookmark forKey:kBookmarkKey];
    astral_ios_sync_chosen();
}

- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller
{
    gPicker = nil;
}
@end

NSString *AstralSyncFolderName(void)
{
    return ResolvedFolder().lastPathComponent;
}

void AstralChooseSyncFolder(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIDocumentPickerViewController *picker = [[UIDocumentPickerViewController alloc] initForOpeningContentTypes:@[ UTTypeFolder ]];
        gPicker = [AstralSyncPicker new];
        picker.delegate = gPicker;
        UIViewController *top = AstralPhoenixWindow().rootViewController;
        while (top.presentedViewController) top = top.presentedViewController;
        [top presentViewController:picker animated:YES completion:nil];
    });
}

void AstralForgetSyncFolder(void)
{
    [NSUserDefaults.standardUserDefaults removeObjectForKey:kBookmarkKey];
}

NSString *AstralReadSyncFile(void)
{
    return WithFolder(^id(NSURL *file) {
        __block NSString *text = nil;
        [[[NSFileCoordinator alloc] initWithFilePresenter:nil]
            coordinateReadingItemAtURL:file
                               options:0
                                 error:nil
                            byAccessor:^(NSURL *url) {
                                text = [NSString stringWithContentsOfURL:url encoding:NSUTF8StringEncoding error:nil];
                            }];
        return text;
    });
}

BOOL AstralWriteSyncFile(NSString *contents)
{
    NSNumber *written = WithFolder(^id(NSURL *file) {
        __block BOOL done = NO;
        [[[NSFileCoordinator alloc] initWithFilePresenter:nil] coordinateWritingItemAtURL:file
                                                                                  options:NSFileCoordinatorWritingForReplacing
                                                                                    error:nil
                                                                               byAccessor:^(NSURL *url) {
                                                                                   done = [contents writeToURL:url
                                                                                                    atomically:YES
                                                                                                      encoding:NSUTF8StringEncoding
                                                                                                         error:nil];
                                                                               }];
        return @(done);
    });
    return written.boolValue;
}
