#define _GNU_SOURCE
#include <dlfcn.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/statvfs.h>
#include <sys/stat.h>
#include <sys/resource.h>
#include <signal.h>
#include <unistd.h>

static int (*original_openat)(int, const char *, int, ...);
static int (*original_open64)(const char *, int, ...);
static ssize_t (*original_write)(int, const void *, size_t);
static int written[3];
static int snapshot_fd = -1;
static int source_changed;
static unsigned long long source_read;

__attribute__((constructor)) static void initialize(void) {
    original_openat = dlsym(RTLD_NEXT, "openat");
    original_open64 = dlsym(RTLD_NEXT, "open64");
    original_write = dlsym(RTLD_NEXT, "write");
    const char *phase = getenv("ZIPELO_SPOOL_PHASE");
    if (phase && !strcmp(phase, "quota")) {
        struct rlimit limit = {65536, 65536};
        signal(SIGXFSZ, SIG_IGN);
        if (setrlimit(RLIMIT_FSIZE, &limit)) _exit(99);
    }
}
static int location(const char *path) {
    if (strstr(path, "/cache/")) return 0;
    if (strstr(path, "/archive/")) return 1;
    if (strstr(path, "/system/")) return 2;
    return -1;
}
static int descriptor(int fd, char *path, size_t size) {
    char link[64];
    path[0] = 0;
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t length = readlink(link, path, size - 1);
    if (length < 0) return -1;
    path[length] = 0;
    return location(path);
}
static int affected(int at) {
    const char *mask = getenv("ZIPELO_SPOOL_MASK");
    return at >= 0 && mask && (atoi(mask) & (1 << at));
}
static void trace(int at) {
    const char *path = getenv("ZIPELO_SPOOL_TRACE");
    if (!path || at < 0) return;
    int fd = original_openat(AT_FDCWD, path, O_CREAT | O_WRONLY | O_APPEND, 0600);
    if (fd >= 0) {
        char value = '0' + at;
        original_write(fd, &value, 1);
        close(fd);
    }
}
static unsigned long long temporary_usage(void) {
    DIR *directory = opendir("/proc/self/fd");
    if (!directory) _exit(98);
    struct dirent *entry;
    struct stat seen[256];
    size_t count = 0;
    unsigned long long used = 0;
    while ((entry = readdir(directory))) {
        if (entry->d_name[0] == '.') continue;
        int fd = atoi(entry->d_name);
        char path[4096];
        struct stat value;
        if (descriptor(fd, path, sizeof(path)) != 2 || fstat(fd, &value)
                || !S_ISREG(value.st_mode)) continue;
        size_t i;
        for (i = 0; i < count; i++) {
            if (seen[i].st_dev == value.st_dev && seen[i].st_ino == value.st_ino) break;
        }
        if (i < count) continue;
        if (count == 256) _exit(97);
        seen[count++] = value;
        used += value.st_size;
    }
    closedir(directory);
    return used;
}
static void exhausted(void) {
    const char *path = getenv("ZIPELO_SPOOL_TRACE");
    if (!path) return;
    int fd = original_openat(AT_FDCWD, path, O_CREAT | O_WRONLY | O_APPEND, 0600);
    if (fd >= 0) {
        original_write(fd, "F", 1);
        close(fd);
    }
}
#define TEMPORARY_CAPACITY (2ULL * 1024 * 1024)
static int reject(int at, int flags) {
    if ((flags & O_TMPFILE) != O_TMPFILE && !(flags & O_CREAT)) return 0;
    trace(at);
    const char *phase = getenv("ZIPELO_SPOOL_PHASE");
    if (affected(at) && at < 2 && phase && !strcmp(phase, "tiny-tmpfs")) {
        errno = EDQUOT;
        return 1;
    }
    if (affected(at) && phase && (!strcmp(phase, "create") || !strcmp(phase, "denied"))) {
        errno = !strcmp(phase, "denied") ? EACCES : EDQUOT;
        return 1;
    }
    if (getenv("ZIPELO_SPOOL_NAMED") && (flags & O_TMPFILE) == O_TMPFILE) {
        errno = EOPNOTSUPP;
        return 1;
    }
    return 0;
}
int openat(int fd, const char *path, int flags, ...) {
    mode_t mode = 0;
    if ((flags & O_CREAT) || (flags & O_TMPFILE) == O_TMPFILE) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, unsigned int);
        va_end(args);
    }
    char parent[4096], full[8192];
    descriptor(fd, parent, sizeof(parent));
    snprintf(full, sizeof(full), "%s/%s", parent, path);
    if (reject(location(full), flags)) return -1;
    int result = original_openat(fd, path, flags, mode);
    if (result >= 0 && snapshot_fd < 0 && location(full) >= 0
            && ((flags & O_TMPFILE) == O_TMPFILE || (flags & O_CREAT))) {
        snapshot_fd = result;
    }
    return result;
}
int open64(const char *path, int flags, ...) {
    mode_t mode = 0;
    if ((flags & O_CREAT) || (flags & O_TMPFILE) == O_TMPFILE) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, unsigned int);
        va_end(args);
    }
    char full[8192];
    snprintf(full, sizeof(full), "%s/", path);
    if (reject(location(full), flags)) return -1;
    int result = original_open64(path, flags, mode);
    if (result >= 0 && snapshot_fd < 0 && location(full) >= 0
            && ((flags & O_TMPFILE) == O_TMPFILE || (flags & O_CREAT))) {
        snapshot_fd = result;
    }
    return result;
}
ssize_t write(int fd, const void *bytes, size_t count) {
    char path[4096];
    int at = descriptor(fd, path, sizeof(path));
    const char *phase = getenv("ZIPELO_SPOOL_PHASE");
    if (phase && !strcmp(phase, "tiny-tmpfs") && at == 2) {
        struct stat value;
        off_t position = lseek(fd, 0, SEEK_CUR);
        if (position < 0 || fstat(fd, &value)) _exit(96);
        unsigned long long used = temporary_usage();
        unsigned long long room = used < TEMPORARY_CAPACITY ? TEMPORARY_CAPACITY - used : 0;
        unsigned long long end = (unsigned long long)value.st_size + room;
        if ((unsigned long long)position >= end) {
            exhausted();
            errno = ENOSPC;
            return -1;
        }
        if (count > end - position) count = end - position;
    }
    int stage = phase && !strcmp(phase, "stage-write") && fd != snapshot_fd;
    if (affected(at) && phase && (stage || !strcmp(phase, "write")
            || !strcmp(phase, "changed") || !strcmp(phase, "io"))) {
        if (!strcmp(phase, "io")) { errno = EIO; return -1; }
        if (!written[at]++) return original_write(fd, bytes, count < 7 ? count : 7);
        errno = EDQUOT;
        return -1;
    }
    return original_write(fd, bytes, count);
}
int fstatvfs(int fd, struct statvfs *value) {
    int (*original)(int, struct statvfs *) = dlsym(RTLD_NEXT, "fstatvfs");
    int result = original(fd, value);
    char path[4096];
    int at = descriptor(fd, path, sizeof(path));
    const char *phase = getenv("ZIPELO_SPOOL_PHASE");
    int stage = phase && !strcmp(phase, "stage-space") && fd != snapshot_fd;
    if (!result && affected(at) && phase && (stage || !strcmp(phase, "space"))) {
        value->f_frsize = 1;
        value->f_bavail = stage ? 0 : 1024;
    }
    if (!result && affected(at) && phase && !strcmp(phase, "quota")) {
        value->f_frsize = 1;
        value->f_bavail = 65536;
    }
    if (!result && at == 2 && phase && !strcmp(phase, "tiny-tmpfs")) {
        unsigned long long used = temporary_usage();
        value->f_frsize = 1;
        value->f_blocks = TEMPORARY_CAPACITY;
        value->f_bavail = value->f_bfree = used < TEMPORARY_CAPACITY ? TEMPORARY_CAPACITY - used : 0;
    }
    return result;
}

ssize_t read(int fd, void *bytes, size_t count) {
    ssize_t (*original)(int, void *, size_t) = dlsym(RTLD_NEXT, "read");
    ssize_t length = original(fd, bytes, count);
    const char *phase = getenv("ZIPELO_SPOOL_PHASE");
    const char *source = getenv("ZIPELO_SPOOL_SOURCE");
    char path[4096];
    descriptor(fd, path, sizeof(path));
    struct stat data;
    if (length > 0 && source && !strcmp(path, source)) source_read += length;
    int late = phase && !strcmp(phase, "late-changed") && !fstat(fd, &data)
        && source_read > (unsigned long long)data.st_size;
    if (length > 0 && !source_changed && phase && (!strcmp(phase, "changed") || late)
            && source && !strcmp(path, source)) {
        source_changed = 1;
        int output = original_openat(AT_FDCWD, source, O_WRONLY | O_APPEND);
        if (output >= 0) {
            original_write(output, "x", 1);
            close(output);
        }
    }
    return length;
}
