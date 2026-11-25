#pragma once

#include <string>

#include "AstralLaunchGuard.h"
#include "AstralRust.h"

extern "C" {
int astral_exit_record_mark_ended(const char *directory, size_t length);
int astral_exit_record_clear(const char *directory, size_t length);
int astral_exit_record_take(const char *directory, size_t length, int nativeCrash);
}

namespace astral {
class ExitRecord
{
  public:
    explicit ExitRecord(std::string directory) : directory_(std::move(directory)) {}

    bool markEnded() const { return astral_exit_record_mark_ended(directory_.data(), directory_.size()); }
    bool clear() const { return astral_exit_record_clear(directory_.data(), directory_.size()); }
    PreviousExit take(bool nativeCrashRecorded) const
    {
        int previous = astral_exit_record_take(directory_.data(), directory_.size(), nativeCrashRecorded);
        return previous == 1 ? PreviousExit::Crashed : previous == 2 ? PreviousExit::Ended : PreviousExit::Unknown;
    }

  private:
    std::string directory_;
};
} // namespace astral
