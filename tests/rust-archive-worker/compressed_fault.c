#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <unistd.h>

static int regular_calls;

int fstatvfs(int fd, struct statvfs *value) {
    int (*original)(int, struct statvfs *) = dlsym(RTLD_NEXT, "fstatvfs");
    int result = original(fd, value);
    const char *mode = getenv("ZIPELO_COMPRESSED_FAULT");
    struct stat info;
    if (result || !mode || fstat(fd, &info) || !S_ISREG(info.st_mode)) return result;
    if (++regular_calls != 2) return result;
    int marker = open(getenv("ZIPELO_COMPRESSED_MARKER"), O_CREAT | O_EXCL | O_WRONLY, 0600);
    if (marker < 0) { errno = EIO; return -1; }
    close(marker);
    if (!strcmp(mode, "full")) { value->f_bavail = 0; return 0; }
    if (!strcmp(mode, "io")) { errno = EIO; return -1; }
    const char *release = getenv("ZIPELO_COMPRESSED_RELEASE");
    for (int i = 0; i < 1000 && access(release, F_OK); i++) usleep(10000);
    if (access(release, F_OK)) { errno = EIO; return -1; }
    return result;
}
