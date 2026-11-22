#include "PhoenixHostCall.h"

#include <mach/mach.h>
#include <mach/vm_statistics.h>
#include <sys/mman.h>

enum { kChunk = 1 << 20, kMaxSlots = 8 };

static bool isHeapTag(unsigned int tag)
{
    switch (tag) {
    case VM_MEMORY_MALLOC:
    case VM_MEMORY_MALLOC_SMALL:
    case VM_MEMORY_MALLOC_TINY:
    case VM_MEMORY_MALLOC_NANO:
    case VM_MEMORY_MALLOC_MEDIUM:
        return true;
    default:
        return false;
    }
}

static bool isRecord(const AstralHostCallImage *image, const uint64_t *words, size_t index, size_t count)
{
    if (index == 0 || index + 1 >= count || words[index] != image->call) return false;
    uint64_t drop = words[index + 1];
    return words[index - 1] != 0 && drop != image->call && drop >= image->textStart && drop < image->textEnd;
}

static size_t findSlots(const AstralHostCallImage *image, uint64_t *buffer, vm_address_t *slots, uint64_t *contexts)
{
    size_t found = 0;
    vm_address_t address = 0;
    natural_t depth = 0;
    for (;;) {
        vm_size_t size = 0;
        vm_region_submap_info_data_64_t info;
        mach_msg_type_number_t infoCount = VM_REGION_SUBMAP_INFO_COUNT_64;
        if (vm_region_recurse_64(mach_task_self(), &address, &size, &depth, (vm_region_recurse_info_64_t)&info, &infoCount) != KERN_SUCCESS)
            break;
        if (info.is_submap) {
            depth++;
            continue;
        }
        bool readable = (info.protection & (VM_PROT_READ | VM_PROT_WRITE)) == (VM_PROT_READ | VM_PROT_WRITE);
        if (readable && isHeapTag(info.user_tag)) {
            for (vm_size_t offset = 0; offset < size; offset += kChunk) {
                vm_size_t wanted = size - offset < kChunk ? size - offset : kChunk;
                vm_size_t copied = 0;
                if (vm_read_overwrite(mach_task_self(), address + offset, wanted, (vm_address_t)buffer, &copied) != KERN_SUCCESS) continue;
                size_t count = (size_t)(copied / sizeof(uint64_t));
                for (size_t index = 1; index + 1 < count; index++) {
                    if (!isRecord(image, buffer, index, count)) continue;
                    if (found == kMaxSlots) return 0;
                    slots[found] = address + offset + index * sizeof(uint64_t);
                    contexts[found] = buffer[index - 1];
                    found++;
                }
            }
        }
        address += size;
    }
    return found;
}

size_t astral_host_call_swap(const AstralHostCallImage *image, AstralHostCall replacement, void **context)
{
    void *mapping = mmap(NULL, kChunk, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, VM_MAKE_TAG(VM_MEMORY_APPLICATION_SPECIFIC_1), 0);
    if (mapping == MAP_FAILED) return 0;
    uint64_t *buffer = mapping;
    vm_address_t slots[kMaxSlots];
    uint64_t contexts[kMaxSlots];
    size_t found = findSlots(image, buffer, slots, contexts);
    munmap(mapping, kChunk);
    for (size_t index = 0; index < found; index++) {
        uint64_t expected = image->call;
        __atomic_compare_exchange_n((uint64_t *)slots[index], &expected, (uint64_t)(uintptr_t)replacement, false, __ATOMIC_SEQ_CST,
                                    __ATOMIC_SEQ_CST);
    }
    if (found && context) *context = (void *)(uintptr_t)contexts[found - 1];
    return found;
}
