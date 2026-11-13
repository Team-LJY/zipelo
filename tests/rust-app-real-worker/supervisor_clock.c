#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <string.h>
#include <time.h>

int clock_gettime(clockid_t id, struct timespec *value) {
    int (*original)(clockid_t, struct timespec *) = dlsym(RTLD_NEXT, "clock_gettime");
    int result = original(id, value);
    char name[16] = {0};
    pthread_getname_np(pthread_self(), name, sizeof(name));
    if (result == 0 && id == CLOCK_MONOTONIC && !strcmp(name, "zipelo-worker-o")) {
        static _Thread_local time_t start;
        if (!start) start = value->tv_sec;
        if (value->tv_sec - start >= 2) value->tv_sec += 311;
    }
    return result;
}
