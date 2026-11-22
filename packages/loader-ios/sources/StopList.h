#pragma once
#import <Foundation/Foundation.h>

void AstralClearCachedCatalog(void);
void AstralCacheVerifiedCatalog(NSData *_Nonnull catalog, NSString *_Nonnull signature);
NSString *_Nullable AstralStopListReason(NSString *_Nullable phoenixUUID);
