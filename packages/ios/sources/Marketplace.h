#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef NS_ENUM(NSInteger, AstralCatalogState) {
    AstralCatalogIdle,
    AstralCatalogLoading,
    AstralCatalogReady,
    AstralCatalogFailed,
};

AstralCatalogState AstralCatalogCurrentState(void);
NSDictionary *_Nullable AstralCatalog(void);
NSString *_Nonnull AstralCatalogURL(void);
NSString *_Nullable AstralCatalogError(void);
void AstralCatalogRefresh(void (^_Nonnull changed)(void));
NSString *_Nullable AstralInstallStatus(NSString *_Nonnull packageID);
void AstralInstallPackage(NSDictionary *_Nonnull package, NSString *_Nonnull kind, void (^_Nonnull changed)(void));
BOOL AstralVersionIsNewer(NSString *_Nullable candidate, NSString *_Nullable current);

#ifdef __cplusplus
}
#endif
