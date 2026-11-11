#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>

static ssize_t (*real_write)(int, const void *, size_t);
static int (*real_openat)(int, const char *, int, ...);
static int (*real_open64)(const char *, int, ...);
static int started;

__attribute__((constructor)) static void initialize(void) {
    real_write = dlsym(RTLD_NEXT, "write");
    real_openat = dlsym(RTLD_NEXT, "openat");
    real_open64 = dlsym(RTLD_NEXT, "open64");
}

static int output_file(int fd) {
    char link[64], path[4096];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t count = readlink(link, path, sizeof(path) - 1);
    if (count < 0) return 0;
    path[count] = 0;
    const char *fault = getenv("ZIPELO_TEST_FAULT");
    if (fault && !strcmp(fault, "stage-full")) return strstr(path, "/#") && strstr(path, "(deleted)");
    return strstr(path, "/.zipelo-") != NULL;
}

int openat(int fd, const char *path, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, unsigned int);
        va_end(args);
    }
    const char *fault = getenv("ZIPELO_TEST_FAULT");
    if (fault && !strcmp(fault, "create-full") && !strncmp(path, ".zipelo-", 8)) {
        errno = ENOSPC;
        return -1;
    }
    return real_openat(fd, path, flags, mode);
}

int open64(const char *path, int flags, ...) {
    mode_t mode = 0;
    if ((flags & O_CREAT) || ((flags & O_TMPFILE) == O_TMPFILE)) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, unsigned int);
        va_end(args);
    }
    const char *fault = getenv("ZIPELO_TEST_FAULT");
    if (fault && !strcmp(fault, "fallback") && ((flags & O_TMPFILE) == O_TMPFILE)) {
        int marker = real_openat(AT_FDCWD, getenv("ZIPELO_TEST_MARKER"), O_CREAT | O_WRONLY, 0600);
        if (marker >= 0) close(marker);
        errno = EOPNOTSUPP;
        return -1;
    }
    return real_open64(path, flags, mode);
}

ssize_t write(int fd, const void *bytes, size_t count) {
    const char *fault = getenv("ZIPELO_TEST_FAULT");
    if (!fault || !output_file(fd)) return real_write(fd, bytes, count);
    if (!strcmp(fault, "full") || !strcmp(fault, "stage-full")) {
        if (!started++) return real_write(fd, bytes, count < 7 ? count : 7);
        errno = ENOSPC;
        return -1;
    }
    if (!strcmp(fault, "pause") && !started++) {
        const char *marker = getenv("ZIPELO_TEST_MARKER");
        const char *release = getenv("ZIPELO_TEST_RELEASE");
        int signal_fd = real_openat(AT_FDCWD, marker, O_CREAT | O_EXCL | O_WRONLY, 0600);
        if (signal_fd < 0) { errno = EIO; return -1; }
        close(signal_fd);
        for (int i = 0; i < 1000 && access(release, F_OK); i++) usleep(10000);
        if (access(release, F_OK)) { errno = EIO; return -1; }
    }
    return real_write(fd, bytes, count);
}
