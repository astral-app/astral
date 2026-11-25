#pragma once

#include <cstdint>
#include <optional>
#include <string>

#include "AstralRust.h"

extern "C" {
int astral_is_out_of_space(int error);
void astral_note_write_failure(int error, const char *path, size_t length, int64_t at);
int astral_disk_full_note(int64_t *at, AstralRustString *file);
void astral_clear_disk_full_note(void);
}

namespace astral {
struct DiskFullNote {
    int64_t at = 0;
    std::string file;
};

inline bool isOutOfSpace(int error)
{
    return astral_is_out_of_space(error);
}

inline void noteWriteFailure(int error, const std::string &path, int64_t at)
{
    astral_note_write_failure(error, path.data(), path.size(), at);
}

inline std::optional<DiskFullNote> diskFullNote()
{
    DiskFullNote note;
    AstralRustString file{};
    if (!astral_disk_full_note(&note.at, &file)) return std::nullopt;
    note.file = rust::take(file);
    return note;
}

inline void clearDiskFullNote()
{
    astral_clear_disk_full_note();
}
} // namespace astral
