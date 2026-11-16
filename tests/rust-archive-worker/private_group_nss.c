#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>

int open64(const char *path, int flags, ...) {
    int (*original)(const char *, int, ...) = dlsym(RTLD_NEXT, "open64");
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, unsigned int);
        va_end(args);
    }
    if (!strcmp(path, "/etc/passwd")) {
        path = getenv("ZIPELO_GROUP_PASSWD");
    } else if (!strcmp(path, "/etc/nsswitch.conf")) {
        path = getenv("ZIPELO_GROUP_NSS");
    }
    if (!path) {
        errno = EACCES;
        return -1;
    }
    return original(path, flags, mode);
}

int getgrgid_r(gid_t gid, struct group *group, char *buffer, size_t length,
        struct group **result) {
    (void)buffer;
    (void)length;
    const char *phase = getenv("ZIPELO_GROUP_CASE");
    *result = NULL;
    if (!phase || !strcmp(phase, "group-error")) {
        return EIO;
    }
    if (!strcmp(phase, "group-missing")) {
        return 0;
    }
    static char *empty[] = {NULL};
    static char *shared[] = {"other", NULL};
    *group = (struct group){
        .gr_name = "fixture",
        .gr_passwd = "x",
        .gr_gid = !strcmp(phase, "group-wrong-gid") ? gid + 1 : gid,
        .gr_mem = !strcmp(phase, "group-null-members") ? NULL :
            (!strcmp(phase, "supplementary-member") ? shared : empty)
    };
    *result = group;
    return 0;
}
