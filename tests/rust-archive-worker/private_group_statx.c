#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

typedef int (*statx_function)(int, const char *, int, unsigned int, struct statx *);

int main(void) {
    void *libc = dlopen("libc.so.6", RTLD_NOW);
    if (!libc) return 1;
    statx_function original = dlsym(libc, "statx");
    statx_function intercepted = dlsym(RTLD_DEFAULT, "statx");
    if (!original || !intercepted || original == intercepted) return 1;
    int fd = open(".", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (fd < 0) return 1;
    const char *paths[] = {NULL, "", "."};
    const int flags[] = {0, AT_EMPTY_PATH};
    int result = 0;
    for (size_t i = 0; i < sizeof(paths) / sizeof(paths[0]); i++) {
        for (size_t j = 0; j < sizeof(flags) / sizeof(flags[0]); j++) {
            struct statx expected = {0};
            struct statx actual = {0};
            errno = 0;
            int expected_result = original(fd, paths[i], flags[j], STATX_BASIC_STATS, &expected);
            int expected_errno = errno;
            errno = 0;
            int actual_result = intercepted(fd, paths[i], flags[j], STATX_BASIC_STATS, &actual);
            int actual_errno = errno;
            if (actual_result != expected_result || actual_errno != expected_errno ||
                    (actual_result == 0 && (actual.stx_mask != expected.stx_mask ||
                        actual.stx_mode != expected.stx_mode || actual.stx_ino != expected.stx_ino ||
                        actual.stx_size != expected.stx_size))) {
                fprintf(stderr, "path=%s flags=%d: result=%d errno=%d, expected result=%d errno=%d\n",
                    paths[i] ? paths[i] : "NULL", flags[j], actual_result, actual_errno,
                    expected_result, expected_errno);
                result = 1;
            }
        }
    }
    close(fd);
    dlclose(libc);
    return result;
}
