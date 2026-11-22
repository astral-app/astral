#include "AstralLaunchReport.h"
#include "AstralLog.h"

void astral::recordSettingsRecovery(const std::string &recovery)
{
    log(LogLevel::Warning, "astral", recovery);
}
