"""시스템 libarchive를 시험에서만 사용해 7z 상호 운용을 확인함."""
import ctypes as c
import ctypes.util
from pathlib import Path
import sys


def library():
    lib = c.CDLL(ctypes.util.find_library("archive"))
    functions = {
        "archive_write_new": (c.c_void_p, []),
        "archive_read_new": (c.c_void_p, []),
        "archive_entry_new": (c.c_void_p, []),
        "archive_error_string": (c.c_char_p, [c.c_void_p]),
        "archive_write_set_format_7zip": (c.c_int, [c.c_void_p]),
        "archive_write_set_format_option": (c.c_int, [c.c_void_p, c.c_char_p, c.c_char_p, c.c_char_p]),
        "archive_write_open_memory": (c.c_int, [c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]),
        "archive_entry_set_pathname": (None, [c.c_void_p, c.c_char_p]),
        "archive_entry_set_filetype": (None, [c.c_void_p, c.c_uint]),
        "archive_entry_set_perm": (None, [c.c_void_p, c.c_uint]),
        "archive_entry_set_size": (None, [c.c_void_p, c.c_longlong]),
        "archive_entry_set_mtime": (None, [c.c_void_p, c.c_longlong, c.c_long]),
        "archive_write_header": (c.c_int, [c.c_void_p, c.c_void_p]),
        "archive_write_data": (c.c_ssize_t, [c.c_void_p, c.c_void_p, c.c_size_t]),
        "archive_entry_free": (None, [c.c_void_p]),
        "archive_write_close": (c.c_int, [c.c_void_p]),
        "archive_write_free": (c.c_int, [c.c_void_p]),
        "archive_read_support_filter_all": (c.c_int, [c.c_void_p]),
        "archive_read_support_format_7zip": (c.c_int, [c.c_void_p]),
        "archive_read_open_filename": (c.c_int, [c.c_void_p, c.c_char_p, c.c_size_t]),
        "archive_read_next_header": (c.c_int, [c.c_void_p, c.POINTER(c.c_void_p)]),
        "archive_entry_pathname": (c.c_char_p, [c.c_void_p]),
        "archive_read_data": (c.c_ssize_t, [c.c_void_p, c.c_void_p, c.c_size_t]),
        "archive_read_free": (c.c_int, [c.c_void_p]),
    }
    for name, (result, args) in functions.items():
        function = getattr(lib, name)
        function.restype = result
        function.argtypes = args
    return lib


def create(method, entries):
    lib = library()
    archive = lib.archive_write_new()
    buffer = c.create_string_buffer(4 * 1024 * 1024)
    used = c.c_size_t()
    def check(result):
        assert result == 0, lib.archive_error_string(archive)
    try:
        check(lib.archive_write_set_format_7zip(archive))
        check(lib.archive_write_set_format_option(archive, b"7zip", b"compression", method.encode()))
        check(lib.archive_write_open_memory(archive, buffer, len(buffer), c.byref(used)))
        for name, data in entries:
            entry = lib.archive_entry_new()
            try:
                lib.archive_entry_set_pathname(entry, name.encode())
                lib.archive_entry_set_filetype(entry, 0o100000)
                lib.archive_entry_set_perm(entry, 0o600)
                lib.archive_entry_set_size(entry, len(data))
                lib.archive_entry_set_mtime(entry, 0, 0)
                check(lib.archive_write_header(archive, entry))
                assert lib.archive_write_data(archive, data, len(data)) == len(data)
            finally:
                lib.archive_entry_free(entry)
        check(lib.archive_write_close(archive))
        return buffer.raw[:used.value]
    finally:
        lib.archive_write_free(archive)


def verify(path):
    lib = library()
    archive = lib.archive_read_new()
    result = {}
    try:
        assert lib.archive_read_support_filter_all(archive) == 0
        assert lib.archive_read_support_format_7zip(archive) == 0
        assert lib.archive_read_open_filename(archive, str(path).encode(), 65536) == 0
        entry = c.c_void_p()
        while True:
            status = lib.archive_read_next_header(archive, c.byref(entry))
            if status == 1:
                return result
            assert status == 0, lib.archive_error_string(archive)
            name = lib.archive_entry_pathname(entry).decode()
            data = bytearray()
            buffer = c.create_string_buffer(65536)
            while True:
                size = lib.archive_read_data(archive, buffer, len(buffer))
                assert size >= 0, lib.archive_error_string(archive)
                if size == 0:
                    break
                data.extend(buffer.raw[:size])
            result[name] = bytes(data)
    finally:
        lib.archive_read_free(archive)


if __name__ == "__main__":
    root = Path(__file__).resolve().parent / "fixtures/7z"
    payload = b"Zipelo 7z interoperability\n" * 100
    if sys.argv[1:] == ["--generate"]:
        for method in ("lzma1", "lzma2"):
            path = root / f"libarchive-{method}.7z"
            path.write_bytes(create(method, [("folder/hello.txt", payload), ("second", payload[::-1])]))
            assert verify(path) == {"folder/hello.txt": payload, "second": payload[::-1]}
    else:
        for name in ("copy.7z", "lzma.7z", "lzma2.7z", "encoded-solid.7z", "long-match.7z", "lzma2-chunks.7z"):
            assert verify(root / name)
