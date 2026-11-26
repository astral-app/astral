#pragma once

#import <Foundation/Foundation.h>

#ifdef __cplusplus
extern "C" {
#endif

void AstralArmPhoenixAccessUnlock(void);
NSDictionary<NSString *, id> *_Nonnull AstralPhoenixAccessStatus(void);
void AstralPhoenixAccessHealthy(void);

#ifdef __cplusplus
}
#endif
