#include "PhoenixWireInternal.h"

#include <math.h>

int astral_wire_event_value(const uint8_t *bytes, size_t length, double *value)
{
    if (!bytes || !value || length < 5) return 0;
    Cursor cursor = {bytes, length, 0, 0};
    if (wireRead8(&cursor) != 8) return 0;
    uint32_t count = wireRead32(&cursor);
    if (count > kMaxProps) return 0;
    int found = 0;
    double number = 0;
    for (uint32_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t key = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (key != P(value)) {
            wireSkipValue(&cursor, tag, 0);
            continue;
        }
        if (found) return 0;
        if (tag == 3)
            number = (int32_t)wireRead32(&cursor);
        else if (tag == 4 || tag == 5) {
            if (!wireNeed(&cursor, 8)) return 0;
            if (tag == 4) {
                int64_t integer;
                memcpy(&integer, bytes + cursor.offset, 8);
                number = (double)integer;
            } else
                memcpy(&number, bytes + cursor.offset, 8);
            cursor.offset += 8;
        } else
            return 0;
        found = 1;
    }
    if (!found || cursor.failed || cursor.offset != length || !isfinite(number)) return 0;
    *value = number;
    return 1;
}
