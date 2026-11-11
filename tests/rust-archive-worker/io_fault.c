#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/statvfs.h>

static ssize_t (*real_write)(int, const void *, size_t);
static int (*real_openat)(int, const char *, int, ...);
static int (*real_open64)(const char *, int, ...);
static int (*real_fsync)(int);
static int (*real_fstatvfs)(int, struct statvfs *);
static int started;
static int snapshot_fd = -1;

__attribute__((constructor)) static void initialize(void) {
    real_write = dlsym(RTLD_NEXT, "write");
    real_openat = dlsym(RTLD_NEXT, "openat");
    real_open64 = dlsym(RTLD_NEXT, "open64");
    real_fsync = dlsym(RTLD_NEXT, "fsync");
    real_fstatvfs = dlsym(RTLD_NEXT, "fstatvfs");
}

static int output_file(int fd) {
    char link[64], path[4096];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t count = readlink(link, path, sizeof(path) - 1);
    if (count < 0) return 0;
    path[count] = 0;
    const char *fault = getenv("ZIPELO_TEST_FAULT");
    if (fault && (!strcmp(fault, "stage-full") || !strcmp(fault, "codec-full"))) return strstr(path, "/#") && strstr(path, "(deleted)");
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
    if (fault && (!strcmp(fault, "create-full") || !strcmp(fault, "create-denied"))
            && !strncmp(path, ".zipelo-", 8)) {
        errno = !strcmp(fault, "create-full") ? ENOSPC : EACCES;
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
    if (fault && !strcmp(fault, "temp-full")
            && (((flags & O_TMPFILE) == O_TMPFILE) || strstr(path, "/zipelo-"))) {
        errno = ENOSPC;
        return -1;
    }
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
    if (!strcmp(fault, "codec-full")) {
        if (snapshot_fd == -1) snapshot_fd = fd;
        if (fd == snapshot_fd) return real_write(fd, bytes, count);
        if (!started++) return real_write(fd, bytes, count < 7 ? count : 7);
        errno = ENOSPC;
        return -1;
    }
    if (!strcmp(fault, "write-io")) { errno = EIO; return -1; }
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

int fstatvfs(int fd, struct statvfs *value) {
    int result = real_fstatvfs(fd, value);
    const char *fault = getenv("ZIPELO_TEST_FAULT");
    if (result || !fault || strncmp(fault, "space-", 6)) return result;
    struct stat st;
    if (fstat(fd, &st)) return -1;
    int affected = !strcmp(fault, "space-error") || !strcmp(fault, "space-combined")
        || (!strcmp(fault, "space-stage") && S_ISREG(st.st_mode))
        || (!strcmp(fault, "space-destination") && S_ISDIR(st.st_mode));
    if (!affected) return result;
    int marker = real_openat(AT_FDCWD, getenv("ZIPELO_TEST_MARKER"), O_CREAT | O_WRONLY, 0600);
    if (marker >= 0) close(marker);
    if (!strcmp(fault, "space-error")) { errno = EIO; return -1; }
    value->f_bavail = !strcmp(fault, "space-combined") ? (1536 * 1024) / value->f_frsize : 0;
    return 0;
}

int fsync(int fd) {
    const char *fault = getenv("ZIPELO_TEST_FAULT");
    struct stat st;
    int directory_full = fault && !strcmp(fault, "dir-sync-full")
        && !fstat(fd, &st) && S_ISDIR(st.st_mode);
    if (directory_full || (fault && !strcmp(fault, "sync-full") && output_file(fd))) {
        errno = ENOSPC;
        return -1;
    }
    return real_fsync(fd);
}
