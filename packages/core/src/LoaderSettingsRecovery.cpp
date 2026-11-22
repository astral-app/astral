#include "LoaderLaunchReport.h"
#include "LoaderLog.h"

void loader::recordSettingsRecovery(const std::string &recovery)
{
    log(LogLevel::Warning, "astral", recovery);
}
