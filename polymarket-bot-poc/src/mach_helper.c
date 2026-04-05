#include <mach/mach.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void sample_memory_mach(int64_t* rss, int64_t* vm) {
    struct mach_task_basic_info info;
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO,
                  (task_info_t)&info, &count) == KERN_SUCCESS) {
        *rss = (int64_t)info.resident_size;
        *vm = (int64_t)info.virtual_size;
    }
}

#ifdef __cplusplus
}
#endif
