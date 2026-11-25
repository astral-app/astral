#pragma once

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "AstralRust.h"

extern "C" {
int astral_read_text_file(const char *path, size_t length, AstralRustString *contents);
AstralRustString astral_unreadable_reason(const char *path, size_t length);
int astral_move_aside_unreadable(const char *path, size_t length);
int astral_write_text_file_atomically(const char *path, size_t length, const char *contents, size_t contentsLength);
int astral_copy_file(const char *source, size_t sourceLength, const char *target, size_t targetLength);
int astral_remove_file(const char *path, size_t length);
int astral_is_contained_relative_path(const char *path, size_t length);
int astral_remove_tree(const char *root, size_t rootLength, const char *relative, size_t relativeLength);
uint64_t astral_file_size(const char *path, size_t length);
int astral_file_exists(const char *path, size_t length);
int astral_is_directory(const char *path, size_t length);
int astral_create_directories(const char *path, size_t length);
AstralRustString astral_list_directory(const char *path, size_t length);
AstralRustString astral_join_path(const char *base, size_t baseLength, const char *component, size_t componentLength);
int astral_resolve_inside_directory(const char *directory, size_t directoryLength, const char *relative, size_t relativeLength,
                                    AstralRustString *resolved);
}

namespace astral {
inline std::optional<std::string> readTextFile(const std::string &path)
{
    return rust::takeOptional([&](AstralRustString *into) { return astral_read_text_file(path.data(), path.size(), into); });
}

inline std::string unreadableReason(const std::string &path)
{
    return rust::take(astral_unreadable_reason(path.data(), path.size()));
}

inline bool moveAsideUnreadable(const std::string &path)
{
    return astral_move_aside_unreadable(path.data(), path.size());
}

inline bool writeTextFileAtomically(const std::string &path, const std::string &contents)
{
    return astral_write_text_file_atomically(path.data(), path.size(), contents.data(), contents.size());
}

inline bool copyFile(const std::string &source, const std::string &destination)
{
    return astral_copy_file(source.data(), source.size(), destination.data(), destination.size());
}

inline bool removeFile(const std::string &path)
{
    return astral_remove_file(path.data(), path.size());
}

inline bool isContainedRelativePath(const std::filesystem::path &relative)
{
    std::string text = relative.string();
    return astral_is_contained_relative_path(text.data(), text.size());
}

inline bool removeTree(const std::string &root, const std::string &relative)
{
    return astral_remove_tree(root.data(), root.size(), relative.data(), relative.size());
}

inline uint64_t fileSize(const std::string &path)
{
    return astral_file_size(path.data(), path.size());
}

inline bool fileExists(const std::string &path)
{
    return astral_file_exists(path.data(), path.size());
}

inline bool isDirectory(const std::string &path)
{
    return astral_is_directory(path.data(), path.size());
}

inline bool createDirectories(const std::string &path)
{
    return astral_create_directories(path.data(), path.size());
}

inline std::vector<std::string> listDirectory(const std::string &path)
{
    return rust::takeList(astral_list_directory(path.data(), path.size()));
}

inline std::string joinPath(const std::string &base, const std::string &component)
{
    return rust::take(astral_join_path(base.data(), base.size(), component.data(), component.size()));
}

inline std::optional<std::string> resolveInsideDirectory(const std::string &directory, const std::string &relative)
{
    return rust::takeOptional([&](AstralRustString *into) {
        return astral_resolve_inside_directory(directory.data(), directory.size(), relative.data(), relative.size(), into);
    });
}
} // namespace astral
