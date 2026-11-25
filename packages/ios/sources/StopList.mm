#import "StopList.h"

#include "AstralMarketplace.h"

NSString *AstralStopListReason(NSString *phoenixUUID)
{
    if (!phoenixUUID.length) return nil;
    auto reason = astral::marketplace::stopListReason(phoenixUUID.UTF8String);
    return reason ? [NSString stringWithUTF8String:reason->c_str()] : nil;
}
