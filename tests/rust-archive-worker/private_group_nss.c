#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <pwd.h>
#include <unistd.h>
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
#include <dirent.h>
#include <limits.h>
#include <stddef.h>
#include <stdio.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>

static const char *redirect(const char *path, char *buffer, size_t length) {
    if (!path) {
        return path;
    }
    const char *base = NULL;
    const char *tail = NULL;
    if (!strncmp(path, "/var/lib/sss", 12) && (!path[12] || path[12] == '/')) {
        base = getenv("ZIPELO_GROUP_SSS");
        tail = path + 12;
    } else {
        const char *paths[] = {"/etc/userdb", "/run/userdb", "/run/host/userdb",
            "/usr/local/lib/userdb", "/usr/lib/userdb", "/run/host", "/run/systemd/userdb"};
        const char *names[] = {"/d0", "/d1", "/host/userdb", "/d3", "/d4", "/host", "/s"};
        for (size_t i = 0; i < sizeof(paths) / sizeof(paths[0]); i++) {
            size_t n = strlen(paths[i]);
            if (!strncmp(path, paths[i], n) && (!path[n] || path[n] == '/')) {
                base = getenv("ZIPELO_GROUP_USERDB");
                int size = snprintf(buffer, length, "%s%s%s", base ? base : "", names[i], path + n);
                if (!base || size < 0 || (size_t)size >= length) {
                    errno = EACCES;
                    return NULL;
                }
                return buffer;
            }
        }
        return path;
    }
    int n = snprintf(buffer, length, "%s%s", base ? base : "", tail);
    if (!base || n < 0 || (size_t)n >= length) {
        errno = EACCES;
        return NULL;
    }
    return buffer;
}

int private_group_statx(int fd, const char *path, int flags, unsigned int mask,
    struct statx *info) __asm__("statx");

int private_group_statx(int fd, const char *path, int flags, unsigned int mask, struct statx *info) {
    int (*original)(int, const char *, int, unsigned int, struct statx *) = dlsym(RTLD_NEXT, "statx");
    const char *phase = getenv("ZIPELO_GROUP_CASE");
    if (path && phase && ((!strcmp(phase, "sss-probe-error") && !strncmp(path, "/var/lib/sss", 12)) ||
            (!strcmp(phase, "userdb-probe-error") && !strcmp(path, "/etc/userdb")))) {
        errno = EACCES;
        return -1;
    }
    char buffer[PATH_MAX];
    const char *mapped = redirect(path, buffer, sizeof(buffer));
    return mapped || !path ? original(fd, mapped, flags, mask, info) : -1;
}

DIR *opendir(const char *path) {
    DIR *(*original)(const char *) = dlsym(RTLD_NEXT, "opendir");
    char buffer[PATH_MAX];
    const char *mapped = redirect(path, buffer, sizeof(buffer));
    return mapped ? original(mapped) : NULL;
}

int connect(int fd, const struct sockaddr *address, socklen_t length) {
    int (*original)(int, const struct sockaddr *, socklen_t) = dlsym(RTLD_NEXT, "connect");
    const struct sockaddr_un *un = (const struct sockaddr_un *)address;
    if (address->sa_family != AF_UNIX || strncmp(un->sun_path, "/run/systemd/userdb/", 20)) {
        return original(fd, address, length);
    }
    char buffer[PATH_MAX];
    const char *mapped = redirect(un->sun_path, buffer, sizeof(buffer));
    if (!mapped) return -1;
    char *slash = strrchr(buffer, '/');
    if (!slash) return -1;
    *slash = 0;
    int parent = open(buffer, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (parent < 0) return -1;
    struct sockaddr_un redirected = {.sun_family = AF_UNIX};
    int n = snprintf(redirected.sun_path, sizeof(redirected.sun_path), "/proc/self/fd/%d/%s", parent, slash + 1);
    int result = -1;
    if (n >= 0 && (size_t)n < sizeof(redirected.sun_path)) {
        result = original(fd, (const struct sockaddr *)&redirected, offsetof(struct sockaddr_un, sun_path) + n + 1);
    }
    close(parent);
    return result;
}

#include <sys/xattr.h>

ssize_t fgetxattr(int fd, const char *name, void *value, size_t size) {
    ssize_t (*original)(int, const char *, void *, size_t) = dlsym(RTLD_NEXT, "fgetxattr");
    const char *phase = getenv("ZIPELO_GROUP_CASE");
    if (!phase || strcmp(name, "system.posix_acl_access")) {
        return original(fd, name, value, size);
    }
    const char *errors[] = {"acl-enodata", "acl-enotsup", "acl-eacces", "acl-eio", "acl-erange"};
    const int codes[] = {ENODATA, ENOTSUP, EACCES, EIO, ERANGE};
    for (size_t i = 0; i < sizeof(codes) / sizeof(codes[0]); i++) {
        if (!strcmp(phase, errors[i])) {
            errno = codes[i];
            return -1;
        }
    }
    if (!strncmp(phase, "acl-invalid-", 12) || !strcmp(phase, "acl-truncated") || !strcmp(phase, "acl-unknown-tag")) {
        unsigned char bytes[] = {2, 0, 0, 0,
            1, 0, 7, 0, 255, 255, 255, 255,
            4, 0, 7, 0, 255, 255, 255, 255,
            32, 0, 5, 0, 255, 255, 255, 255};
        if (!strcmp(phase, "acl-invalid-version")) bytes[0] = 3;
        if (!strcmp(phase, "acl-invalid-permissions")) bytes[6] = 8;
        if (!strcmp(phase, "acl-invalid-id")) bytes[8] = 0;
        if (!strcmp(phase, "acl-unknown-tag")) bytes[4] = 64;
        size_t length = !strcmp(phase, "acl-truncated") ? sizeof(bytes) - 1 : sizeof(bytes);
        if (size < length) { errno = ERANGE; return -1; }
        memcpy(value, bytes, length);
        return (ssize_t)length;
    }
    return original(fd, name, value, size);
}
