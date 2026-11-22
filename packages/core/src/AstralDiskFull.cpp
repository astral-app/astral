#include "AstralDiskFull.h"

#include <cerrno>
#include <filesystem>
#include <mutex>

namespace astral {
static std::mutex noteLock;
static std::optional<DiskFullNote> latestNote;

bool isOutOfSpace(int error)
{
    return error == ENOSPC || error == EDQUOT;
}

void noteWriteFailure(int error, const std::string &path, int64_t at)
{
    if (!isOutOfSpace(error)) {
        return;
    }

    std::lock_guard<std::mutex> guard(noteLock);
    latestNote = DiskFullNote{at, std::filesystem::path(path).filename().string()};
}

std::optional<DiskFullNote> diskFullNote()
{
    std::lock_guard<std::mutex> guard(noteLock);
    return latestNote;
}

void clearDiskFullNote()
{
    std::lock_guard<std::mutex> guard(noteLock);
    latestNote.reset();
}
} // namespace astral
