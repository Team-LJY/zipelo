#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int archive_fd = -1;
static int finished;
static ssize_t (*original_write)(int, const void *, size_t);
static off64_t (*original_seek)(int, off64_t, int);

__attribute__((constructor)) static void initialize(void) {
    original_write = dlsym(RTLD_NEXT, "write");
    original_seek = dlsym(RTLD_NEXT, "lseek64");
}

ssize_t write(int fd, const void *data, size_t count) {
    const unsigned char *bytes = data;
    if (archive_fd < 0 && count == 32 && !memcmp(bytes, (unsigned char[32]){0}, 32)) {
        archive_fd = fd;
        return original_write(fd, data, count);
    }
    const char *fault = getenv("ZIPELO_7Z_FAULT");
    if (fd == archive_fd && fault) {
        if (!strcmp(fault, "encode-full")) { errno = ENOSPC; return -1; }
        if (count == 1 && bytes[0] == 0) {
            finished = 1;
            if (!strcmp(fault, "finish-full")) { errno = ENOSPC; return -1; }
        }
        if (!strcmp(fault, "header-write") && finished && count > 1 && bytes[0] == 1) {
            errno = EIO; return -1;
        }
        if (!strcmp(fault, "header-patch") && count == 32 && !memcmp(bytes, "7z", 2)) {
            errno = EIO; return -1;
        }
    }
    return original_write(fd, data, count);
}

off64_t lseek64(int fd, off64_t offset, int whence) {
    const char *fault = getenv("ZIPELO_7Z_FAULT");
    if (fd == archive_fd && finished && fault && !strcmp(fault, "header-seek") && offset == 0 && whence == SEEK_SET) {
        errno = EIO; return -1;
    }
    return original_seek(fd, offset, whence);
}
