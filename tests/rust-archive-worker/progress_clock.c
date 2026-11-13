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
        if (mode && !strcmp(mode, "clock-stall")) {
            atomic_fetch_add(&elapsed, 60);
        } else if (mode && !strcmp(mode, "clock-progress")) {
            atomic_fetch_add(&elapsed, 10);
        }
    }
    return result;
}

ssize_t write(int fd, const void *buffer, size_t count) {
    ssize_t (*original)(int, const void *, size_t) = dlsym(RTLD_NEXT, "write");
    ssize_t result = original(fd, buffer, count);
    if (result >= 30 && count >= 30 && !memcmp(buffer, "PK\003\004", 4)) {
        const char *mode = getenv("ZIPELO_TEST_FAULT");
        if (mode && !strcmp(mode, "clock-create-progress")) {
            atomic_fetch_add(&elapsed, 10);
        } else if (mode && !strcmp(mode, "clock-create-stall")) {
            atomic_fetch_add(&elapsed, 60);
        }
    }
    return result;
}
