#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static atomic_long elapsed;

int clock_gettime(clockid_t id, struct timespec *value) {
    int (*original)(clockid_t, struct timespec *) = dlsym(RTLD_NEXT, "clock_gettime");
    int result = original(id, value);
    if (result == 0 && id == CLOCK_MONOTONIC) {
        value->tv_sec += atomic_load(&elapsed);
    }
    return result;
}

int fsync(int fd) {
    int (*original)(int) = dlsym(RTLD_NEXT, "fsync");
    int result = original(fd);
    if (result == 0) {
        const char *mode = getenv("ZIPELO_TEST_FAULT");
        atomic_fetch_add(&elapsed, mode && !strcmp(mode, "clock-stall") ? 60 : 10);
    }
    return result;
}
