#pragma once

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

extern "C" {
void astral_navigator_configure(void (*deliver)(const uint8_t *bytes, size_t length),
                                void (*withWire)(void (*work)(void *context), void *context));
int astral_navigator_handles(const char *identifier, size_t length);
void astral_navigator_tap(const char *identifier, size_t length);
void astral_navigator_slide(const char *identifier, size_t length, double fraction);
void astral_navigator_slide_position(const char *identifier, size_t length, double position);
int astral_navigator_back(void);
void astral_navigator_refresh(void);
int astral_navigator_open(void);
}

namespace astral::navigator {

using Deliver = std::function<void(std::vector<uint8_t> bytes)>;
using WithWire = std::function<void(const std::function<void()> &)>;

inline Deliver &deliverer()
{
    static Deliver stored;
    return stored;
}

inline WithWire &wireLock()
{
    static WithWire stored;
    return stored;
}

inline void configure(Deliver deliver, WithWire withWire)
{
    deliverer() = std::move(deliver);
    wireLock() = std::move(withWire);
    astral_navigator_configure(
        [](const uint8_t *bytes, size_t length) {
            if (deliverer()) deliverer()(std::vector<uint8_t>(bytes, bytes + length));
        },
        [](void (*work)(void *context), void *context) {
            if (wireLock())
                wireLock()([&] { work(context); });
            else
                work(context);
        });
}

inline bool handles(const std::string &identifier)
{
    return astral_navigator_handles(identifier.data(), identifier.size());
}

inline void tap(const std::string &identifier)
{
    astral_navigator_tap(identifier.data(), identifier.size());
}

inline void slide(const std::string &identifier, double fraction)
{
    astral_navigator_slide(identifier.data(), identifier.size(), fraction);
}

inline void slidePosition(const std::string &identifier, double position)
{
    astral_navigator_slide_position(identifier.data(), identifier.size(), position);
}

inline bool back()
{
    return astral_navigator_back();
}

inline void refresh()
{
    astral_navigator_refresh();
}

inline bool open()
{
    return astral_navigator_open();
}

} // namespace astral::navigator
