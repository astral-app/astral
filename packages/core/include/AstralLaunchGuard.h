#pragma once

#include <string>

#include "AstralRust.h"

extern "C" {
typedef struct {
    int failures;
    int rollBack;
    int enterRecovery;
} AstralLaunchVerdict;

AstralLaunchVerdict astral_launch_guard_begin(const char *directory, size_t length, int previous);
int astral_launch_guard_succeed(const char *directory, size_t length);
int astral_launch_guard_mark_frozen(const char *directory, size_t length);
int astral_launch_guard_mark_hidden(const char *directory, size_t length);
int astral_launch_guard_mark_shown(const char *directory, size_t length);
AstralRustString astral_launch_roll_back_notice(int failures);
AstralRustString astral_launch_recovery_notice(int failures);
}

namespace astral {
enum class PreviousExit {
    Unknown,
    Crashed,
    Ended,
};

struct LaunchVerdict {
    int failures = 0;
    bool rollBack = false;
    bool enterRecovery = false;
};

class LaunchGuard
{
  public:
    static constexpr const char *kFile = "launch-guard.json";
    static constexpr int kRollBackAfter = 2;
    static constexpr int kRecoveryAfter = 3;

    explicit LaunchGuard(std::string directory) : directory_(std::move(directory)) {}

    LaunchVerdict begin(PreviousExit previous = PreviousExit::Unknown) const
    {
        AstralLaunchVerdict verdict = astral_launch_guard_begin(directory_.data(), directory_.size(), static_cast<int>(previous));
        return {verdict.failures, verdict.rollBack != 0, verdict.enterRecovery != 0};
    }

    bool succeed() const { return astral_launch_guard_succeed(directory_.data(), directory_.size()); }
    bool markFrozen() const { return astral_launch_guard_mark_frozen(directory_.data(), directory_.size()); }
    bool markHidden() const { return astral_launch_guard_mark_hidden(directory_.data(), directory_.size()); }
    bool markShown() const { return astral_launch_guard_mark_shown(directory_.data(), directory_.size()); }

  private:
    std::string directory_;
};

inline std::string launchRollBackNotice(int failures)
{
    return rust::take(astral_launch_roll_back_notice(failures));
}

inline std::string launchRecoveryNotice(int failures)
{
    return rust::take(astral_launch_recovery_notice(failures));
}
} // namespace astral
