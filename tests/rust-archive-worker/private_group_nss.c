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

void *dlopen(const char *name, int flags) {
    void *(*original)(const char *, int) = dlsym(RTLD_NEXT, "dlopen");
    const char *phase = getenv("ZIPELO_GROUP_CASE");
    if (name && !strcmp(name, "libnss_systemd.so.2")) {
        if (phase && !strcmp(phase, "systemd-load-error")) {
            return NULL;
        }
        if (phase && !strcmp(phase, "systemd-symbol-error")) {
            return original("libm.so.6", flags);
        }
        name = getenv("LD_PRELOAD");
    }
    return original(name, flags);
}

static int systemd_record;

int _nss_systemd_setpwent(int stayopen) {
    (void)stayopen;
    systemd_record = 0;
    const char *phase = getenv("ZIPELO_GROUP_CASE");
    return phase && !strcmp(phase, "systemd-set-error") ? -1 : 1;
}

int _nss_systemd_endpwent(void) {
    return 1;
}

int _nss_systemd_getpwent_r(struct passwd *account, char *buffer, size_t length,
        int *error) {
    (void)buffer;
    const char *phase = getenv("ZIPELO_GROUP_CASE");
    if (!phase || !strcmp(phase, "systemd-enumeration-error") ||
            (systemd_record && !strcmp(phase, "systemd-partial-error"))) {
        *error = EIO;
        return -1;
    }
    if ((!strcmp(phase, "systemd-large-record") && length < 32768) ||
            !strcmp(phase, "systemd-buffer-limit")) {
        *error = ERANGE;
        return -2;
    }
    if (systemd_record || !strcmp(phase, "systemd-personal")) {
        return 0;
    }
    systemd_record = 1;
    *account = (struct passwd){
        .pw_name = "systemd-fixture",
        .pw_passwd = "x",
        .pw_uid = geteuid() + 1,
        .pw_gid = getegid() +
            ((strcmp(phase, "systemd-dynamic-shared") &&
              strcmp(phase, "systemd-homed-shared")) ? 1 : 0),
        .pw_gecos = "",
        .pw_dir = "/",
        .pw_shell = "/bin/false"
    };
    return 1;
}
