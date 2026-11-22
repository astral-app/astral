#pragma once

#include <string>

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

    explicit LaunchGuard(std::string directory);

    LaunchVerdict begin(PreviousExit previous = PreviousExit::Unknown) const;
    bool succeed() const;
    bool markFrozen() const;
    bool markHidden() const;
    bool markShown() const;

  private:
    std::string path() const;

    std::string directory_;
};

std::string launchRollBackNotice(int failures);
std::string launchRecoveryNotice(int failures);
} // namespace astral
