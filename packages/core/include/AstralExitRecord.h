#pragma once

#include <string>

#include "AstralLaunchGuard.h"

namespace astral {
class ExitRecord
{
  public:
    static constexpr const char *kFile = "exit-record";

    explicit ExitRecord(std::string directory);

    bool markEnded() const;
    bool clear() const;
    PreviousExit take(bool nativeCrashRecorded) const;

  private:
    std::string path() const;

    std::string directory_;
};
} // namespace astral
