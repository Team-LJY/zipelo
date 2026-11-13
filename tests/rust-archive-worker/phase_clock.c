#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <time.h>
#include <unistd.h>

static atomic_long elapsed;
static atomic_int directories;
static atomic_int existing_files;
static int (*real_openat)(int, const char *, int, ...);
static ssize_t (*real_read)(int, void *, size_t);
static ssize_t (*real_write)(int, const void *, size_t);
static int (*real_mkdirat)(int, const char *, mode_t);
static int (*real_linkat)(int, const char *, int, const char *, int);
static int (*real_renameat)(int, const char *, int, const char *);
static int (*real_unlinkat)(int, const char *, int);
static int (*real_fsync)(int);
static int (*real_fstatvfs)(int, struct statvfs *);

__attribute__((constructor)) static void initialize(void) {
    real_openat = dlsym(RTLD_NEXT, "openat");
    real_read = dlsym(RTLD_NEXT, "read");
    real_write = dlsym(RTLD_NEXT, "write");
    real_mkdirat = dlsym(RTLD_NEXT, "mkdirat");
    real_linkat = dlsym(RTLD_NEXT, "linkat");
    real_renameat = dlsym(RTLD_NEXT, "renameat");
    real_unlinkat = dlsym(RTLD_NEXT, "unlinkat");
    real_fsync = dlsym(RTLD_NEXT, "fsync");
    real_fstatvfs = dlsym(RTLD_NEXT, "fstatvfs");
}

static void tick(const char *stage) {
    const char *mode = getenv("ZIPELO_TEST_FAULT");
    char expected[80];
    snprintf(expected, sizeof(expected), "phase-%s", stage);
    size_t n = strlen(expected);
    if (!mode || (strcmp(mode, "phase-all") && strcmp(mode, "phase-all-stall")
            && (strncmp(mode, expected, n) || (mode[n] && strcmp(mode + n, "-stall"))))) return;
    int saved = errno;
    atomic_fetch_add(&elapsed, strstr(mode, "-stall") ? 60 : 10);
    const char *marker = getenv("ZIPELO_TEST_MARKER");
    int fd = real_openat(AT_FDCWD, marker, O_CREAT | O_APPEND | O_WRONLY, 0600);
    if (fd >= 0) {
        real_write(fd, stage, strlen(stage));
        real_write(fd, "\n", 1);
        close(fd);
    }
    errno = saved;
}

static int output_fd(int fd) {
    char link[64], path[4096];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    int saved = errno;
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    errno = saved;
    if (n < 0) return 0;
    path[n] = 0;
    return strstr(path, "/output") != NULL;
}

int clock_gettime(clockid_t id, struct timespec *value) {
    int (*original)(clockid_t, struct timespec *) = dlsym(RTLD_NEXT, "clock_gettime");
    int result = original(id, value);
    if (!result && id == CLOCK_MONOTONIC) value->tv_sec += atomic_load(&elapsed);
    return result;
}

int openat(int fd, const char *path, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, unsigned int);
        va_end(args);
    }
    int result = real_openat(fd, path, flags, mode);
    if (!strncmp(path, "entry-", 6) && (flags & O_DIRECTORY) && result >= 0) {
        int n = atomic_fetch_add(&directories, 1);
        tick(n < 8 ? "preflight" : "directory");
    }
    if (strstr(path, " (") && (flags & O_PATH)) tick("rename");
    if (!strncmp(path, "entry-", 6) && (flags & O_PATH)) {
        if (output_fd(fd)) {
            if (result >= 0 && atomic_fetch_add(&existing_files, 1) >= 8) tick("overwrite");
            tick("lookup");
        } else if (result >= 0) {
            tick("source");
        }
    }
    if (result >= 0 && (flags & O_CREAT) && !strncmp(path, ".zipelo-", 8)) tick("create");
    return result;
}

int mkdirat(int fd, const char *path, mode_t mode) {
    int result = real_mkdirat(fd, path, mode);
    if (!result && !strncmp(path, "entry-", 6)) tick("mkdir");
    return result;
}

ssize_t read(int fd, void *buffer, size_t count) {
    ssize_t result = real_read(fd, buffer, count);
    if (result >= 30 && (count == 30 || count == 46)
            && (!memcmp(buffer, "PK\003\004", 4) || !memcmp(buffer, "PK\001\002", 4))) tick("catalog");
    return result;
}

static void written_frame(const unsigned char *bytes, size_t count) {
    static unsigned char header[32];
    static size_t used, remaining;
    static unsigned int kind;
    while (count) {
        if (used < sizeof(header)) {
            size_t n = sizeof(header) - used;
            if (n > count) n = count;
            memcpy(header + used, bytes, n);
            used += n;
            bytes += n;
            count -= n;
            if (used != sizeof(header)) continue;
            if (memcmp(header, "ZAW1", 4)) abort();
            kind = header[6] | (header[7] << 8);
            remaining = header[8] | (header[9] << 8) | (header[10] << 16) | ((size_t)header[11] << 24);
        }
        size_t n = remaining < count ? remaining : count;
        remaining -= n;
        bytes += n;
        count -= n;
        if (!remaining) {
            if (kind == 101) tick("list");
            used = 0;
        }
    }
}

ssize_t write(int fd, const void *buffer, size_t count) {
    ssize_t result = real_write(fd, buffer, count);
    if (result >= 30 && count >= 30 && !memcmp(buffer, "PK\003\004", 4)) tick("header");
    if (fd == STDOUT_FILENO && result > 0) written_frame(buffer, result);
    if (result > 0 && output_fd(fd)) tick("write");
    return result;
}

int linkat(int fd, const char *from, int newfd, const char *to, int flags) {
    int result = real_linkat(fd, from, newfd, to, flags);
    if (!result) tick("publish");
    return result;
}

int renameat(int fd, const char *from, int newfd, const char *to) {
    int result = real_renameat(fd, from, newfd, to);
    if (!result) tick("publish");
    return result;
}

int unlinkat(int fd, const char *path, int flags) {
    int result = real_unlinkat(fd, path, flags);
    if (!result && !strncmp(path, ".zipelo-", 8)) tick("cleanup");
    return result;
}

int fsync(int fd) {
    int result = real_fsync(fd);
    if (!result && output_fd(fd)) tick("sync");
    return result;
}

int fstatvfs(int fd, struct statvfs *value) {
    int result = real_fstatvfs(fd, value);
    struct stat metadata;
    if (!result && !fstat(fd, &metadata) && S_ISDIR(metadata.st_mode) && output_fd(fd)) tick("space");
    return result;
}
