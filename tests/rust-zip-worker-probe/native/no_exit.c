#define _POSIX_C_SOURCE 200809L
#ifndef __linux__
#error "Linux 전용 시험 helper임"
#endif

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

enum { CONFIG_BYTES = 64, WIRE_BYTES = 65938, PROOF_BYTES = 96 };

static int read_exact(int fd, unsigned char *data, size_t size)
{
    while (size != 0) {
        ssize_t count = read(fd, data, size);
        if (count < 0 && errno == EINTR) { continue; }
        if (count <= 0) { return 0; }
        data += count;
        size -= (size_t)count;
    }
    return 1;
}

static int write_exact(int fd, const unsigned char *data, size_t size)
{
    while (size != 0) {
        ssize_t count = write(fd, data, size);
        if (count < 0 && errno == EINTR) { continue; }
        if (count <= 0) { return 0; }
        data += count;
        size -= (size_t)count;
    }
    return 1;
}

static void put32(unsigned char *data, uint32_t value)
{
    for (int index = 0; index < 4; index++) { data[index] = (unsigned char)(value >> (8 * index)); }
}

static void put64(unsigned char *data, uint64_t value)
{
    for (int index = 0; index < 8; index++) { data[index] = (unsigned char)(value >> (8 * index)); }
}

static uint64_t now_ns(void)
{
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value) != 0 || value.tv_sec < 0) { return 0; }
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}

static int stdout_closed(void)
{
    errno = 0;
    return fcntl(STDOUT_FILENO, F_GETFD) == -1 && errno == EBADF;
}

// 같은 pipe의 쓰기 참조만 읽어 확인하며 알 수 없는 descriptor는 닫지 않음.
static int writers(const struct stat *pipe, int expected)
{
    DIR *directory = opendir("/proc/self/fd");
    if (directory == NULL) { return 0; }
    int count = 0;
    int entries = 0;
    int valid = 1;
    for (;;) {
        errno = 0;
        struct dirent *entry = readdir(directory);
        if (entry == NULL) {
            if (errno != 0) { valid = 0; }
            break;
        }
        if (++entries > 4096) { valid = 0; break; }
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) { continue; }
        char *end;
        errno = 0;
        long number = strtol(entry->d_name, &end, 10);
        if (errno != 0 || end == entry->d_name || *end != '\0' || number < 0 || number > INT_MAX) {
            valid = 0;
            break;
        }
        int fd = (int)number;
        struct stat state;
        int flags = fcntl(fd, F_GETFL);
        if (flags == -1 || fstat(fd, &state) != 0) { valid = 0; break; }
        if (S_ISFIFO(state.st_mode) && state.st_dev == pipe->st_dev && state.st_ino == pipe->st_ino &&
            (flags & O_ACCMODE) != O_RDONLY) {
            count++;
            if (expected == 1 && fd != STDOUT_FILENO) { valid = 0; break; }
        }
    }
    if (closedir(directory) != 0) { valid = 0; }
    return valid && count == expected;
}

static int new_file(const char *path)
{
    return open(path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
}

int main(int argc, char **argv)
{
    if (argc != 2 || strcmp(argv[1], "--extract-stream-v2") != 0) { return 71; }
    if (signal(SIGALRM, SIG_DFL) == SIG_ERR) { return 71; }
    alarm(12);
    struct stat pipe;
    int flags = fcntl(STDOUT_FILENO, F_GETFL);
    if (flags == -1 || (flags & O_ACCMODE) != O_WRONLY ||
        fstat(STDOUT_FILENO, &pipe) != 0 || !S_ISFIFO(pipe.st_mode) || !writers(&pipe, 1)) { return 71; }

    int wire = open("native-wire.bin", O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    struct stat input;
    if (wire == -1 || fstat(wire, &input) != 0 || !S_ISREG(input.st_mode) ||
        input.st_size != CONFIG_BYTES + WIRE_BYTES || input.st_uid != geteuid() || input.st_nlink != 1) {
        return 71;
    }
    // stdout를 닫기 전에 필요한 파일을 모두 열어 fd 1 재사용을 피함.
    int proof_file = new_file("native-proof.bin");
    int pid_file = new_file("peer.pid");
    int ready_file = new_file("ready");
    if (proof_file == -1 || pid_file == -1 || ready_file == -1) { return 71; }

    unsigned char config[CONFIG_BYTES];
    unsigned char planned[CONFIG_BYTES];
    if (!read_exact(STDIN_FILENO, config, sizeof(config)) ||
        !read_exact(wire, planned, sizeof(planned)) || memcmp(config, planned, sizeof(config)) != 0 ||
        memcmp(config, "ZXE2\002\000\000\000", 8) != 0) { return 71; }
    unsigned char nonzero = 0;
    for (int index = 8; index < 24; index++) { nonzero |= config[index]; }
    if (nonzero == 0) { return 71; }

    char pid_text[32];
    int pid_size = snprintf(pid_text, sizeof(pid_text), "%ld", (long)getpid());
    if (pid_size <= 0 || (size_t)pid_size >= sizeof(pid_text) ||
        !write_exact(pid_file, (unsigned char *)pid_text, (size_t)pid_size)) { return 71; }
    unsigned char buffer[4096];
    size_t remaining = WIRE_BYTES;
    while (remaining != 0) {
        size_t count = remaining < sizeof(buffer) ? remaining : sizeof(buffer);
        if (!read_exact(wire, buffer, count) || !write_exact(STDOUT_FILENO, buffer, count)) { return 71; }
        remaining -= count;
    }
    if (read(wire, buffer, 1) != 0 || !writers(&pipe, 1)) { return 71; }
    if (close(STDOUT_FILENO) != 0 || !stdout_closed()) { return 71; }
    uint64_t closed = now_ns();
    if (closed == 0 || !writers(&pipe, 0) || !stdout_closed()) { return 71; }
    struct timespec delay = { .tv_sec = 0, .tv_nsec = 50000000 };
    while (nanosleep(&delay, &delay) != 0) {
        if (errno != EINTR) { return 71; }
    }
    uint64_t alive = now_ns();
    if (alive <= closed || alive - closed < UINT64_C(50000000) || !stdout_closed()) { return 71; }

    unsigned char proof[PROOF_BYTES] = {0};
    memcpy(proof, "NXP1", 4);
    put32(proof + 4, 1);
    memcpy(proof + 8, config + 8, 16);
    put32(proof + 24, (uint32_t)getpid());
    put32(proof + 28, (uint32_t)getppid());
    put64(proof + 32, (uint64_t)pipe.st_dev);
    put64(proof + 40, (uint64_t)pipe.st_ino);
    put32(proof + 48, 1);
    put32(proof + 52, 0);
    put32(proof + 56, WIRE_BYTES);
    put32(proof + 60, 1);
    put64(proof + 64, closed);
    put64(proof + 72, alive);
    if (!write_exact(proof_file, proof, sizeof(proof)) ||
        !write_exact(ready_file, (const unsigned char *)"1", 1)) { return 71; }
    if (close(wire) != 0 || close(proof_file) != 0 || close(pid_file) != 0 ||
        close(ready_file) != 0) { return 71; }
    // stdin과 stderr를 유지하고 host 종료 또는 독립 12초 상한을 기다림.
    for (;;) { pause(); }
}
