"""세 도구의 ZIP64 원본과 단일 볼륨 확장 필드 조합을 생성함."""
import ctypes as c
import importlib.util
import io
import json
from pathlib import Path
import struct
import subprocess
import sys
import zipfile

NAME = "payload.bin"
PAYLOAD = b"ZIP64 corpus payload\n"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def python_zip(force=False, pipe=False):
    class Pipe(io.BytesIO):
        def seek(self, *args):
            raise io.UnsupportedOperation()

    output = Pipe() if pipe else io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as archive:
        with archive.open(NAME, "w", force_zip64=force) as target:
            target.write(PAYLOAD)
    return output.getvalue()


def libarchive_zip():
    reference = module(
        "archive_reference",
        Path(__file__).resolve().parents[1] / "rust-archive-worker/seven_zip_reference.py",
    )
    lib = reference.library()
    lib.archive_write_set_format_zip.argtypes = [c.c_void_p]
    lib.archive_write_set_format_zip.restype = c.c_int
    lib.archive_version_string.restype = c.c_char_p
    archive = lib.archive_write_new()
    buffer = c.create_string_buffer(65536)
    used = c.c_size_t()
    entry = lib.archive_entry_new()

    def check(value):
        assert value == 0, lib.archive_error_string(archive)

    try:
        check(lib.archive_write_set_format_zip(archive))
        check(lib.archive_write_set_format_option(archive, b"zip", b"zip64", b"1"))
        check(lib.archive_write_set_format_option(archive, b"zip", b"compression", b"store"))
        check(lib.archive_write_open_memory(archive, buffer, len(buffer), c.byref(used)))
        lib.archive_entry_set_pathname(entry, NAME.encode())
        lib.archive_entry_set_filetype(entry, 0o100000)
        lib.archive_entry_set_perm(entry, 0o600)
        lib.archive_entry_set_size(entry, len(PAYLOAD))
        lib.archive_entry_set_mtime(entry, 0, 0)
        check(lib.archive_write_header(archive, entry))
        assert lib.archive_write_data(archive, PAYLOAD, len(PAYLOAD)) == len(PAYLOAD)
        check(lib.archive_write_close(archive))
        return buffer.raw[:used.value], lib.archive_version_string().decode()
    finally:
        lib.archive_entry_free(entry)
        lib.archive_write_free(archive)


def variant(original, central_mask, end_mask, local64=False):
    # 중앙 확장 필드는 원래 크기, 압축 크기, 오프셋, 디스크 순서임.
    end = bytearray(original[-22:])
    central_at = struct.unpack_from("<I", end, 16)[0]
    body = bytearray(original[:central_at])
    central = bytearray(original[central_at:-22])
    assert struct.unpack_from("<H", central, 30)[0] == 0
    assert struct.unpack_from("<H", central, 32)[0] == 0
    if local64:
        name_len = struct.unpack_from("<H", body, 26)[0]
        packed, size = struct.unpack_from("<II", body, 18)
        struct.pack_into("<II", body, 18, 0xffffffff, 0xffffffff)
        struct.pack_into("<H", body, 28, 20)
        body[30 + name_len:30 + name_len] = struct.pack("<HHQQ", 1, 16, size, packed)
    struct.pack_into("<H", body, 4, 45)
    struct.pack_into("<H", central, 6, 45)
    extra = bytearray()
    for bit, offset, field_type in [(0, 24, "I"), (1, 20, "I"), (2, 42, "I"), (3, 34, "H")]:
        if central_mask & (1 << bit):
            value = struct.unpack_from("<" + field_type, central, offset)[0]
            struct.pack_into("<" + field_type, central, offset, 0xffff if bit == 3 else 0xffffffff)
            extra += struct.pack("<I" if bit == 3 else "<Q", value)
    if extra:
        central += struct.pack("<HH", 1, len(extra)) + extra
        struct.pack_into("<H", central, 30, len(extra) + 4)
    central_at = len(body)
    directory = body + central
    record = struct.pack(
        "<4sQHHIIQQQQ", b"PK\x06\x06", 44, 45, 45, 0, 0, 1, 1, len(central), central_at
    )
    locator = struct.pack("<4sIQI", b"PK\x06\x07", 0, len(directory), 1)
    struct.pack_into("<II", end, 12, len(central), central_at)
    # EOCD의 디스크 두 필드, 항목 수 두 필드, 크기와 오프셋을 독립 확장함.
    for bit, offset, field_type in [(0, 4, "H"), (1, 6, "H"), (2, 8, "H"),
                                    (3, 10, "H"), (4, 12, "I"), (5, 16, "I")]:
        if end_mask & (1 << bit):
            struct.pack_into("<" + field_type, end, offset, 0xffff if field_type == "H" else 0xffffffff)
    return bytes(directory + record + locator + end)


def generate(root):
    root = Path(root)
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    cases = []

    def add(identifier, data, producer, payload=PAYLOAD):
        path = root / (identifier + ".zip")
        path.write_bytes(data)
        with zipfile.ZipFile(path) as reference:
            names = reference.namelist()
            assert len(names) == 1
            assert reference.read(names[0]) == payload, identifier
            assert reference.testzip() is None, identifier
        cases.append((identifier, names[0], payload.hex(), producer))

    original = python_zip()
    add("python-classic", original, "Python zipfile")
    add("python-force-local", python_zip(True), "Python zipfile force_zip64")
    add("python-force-descriptor", python_zip(True, True), "Python zipfile force_zip64 unseekable")
    native, version = libarchive_zip()
    assert b"PK\x06\x07" in native
    add("libarchive-force", native, version + " zip64=1")
    (root / NAME).write_bytes(PAYLOAD)
    subprocess.run(
        ["zip", "-q", "-fz", "infozip-native.zip", NAME],
        cwd=root, capture_output=True, check=True,
    )
    info = (root / "infozip-native.zip").read_bytes()
    assert struct.unpack_from("<II", info, 18) == (0xffffffff, 0xffffffff)
    assert b"PK\x06\x07" in info
    add("infozip-force", info, "Info-ZIP -fz regular file")
    for central in range(16):
        for end in range(64):
            add(f"fields-c{central:02x}-e{end:02x}", variant(original, central, end),
                "Python zipfile + APPNOTE field substitution")
        add(f"local64-c{central:02x}", variant(original, central, 0, True),
            "Python zipfile + local size pair + central field substitution")
        expanded = variant(original, central, 0)
        add(f"central-only-c{central:02x}", expanded[:-98] + expanded[-22:],
            "Python zipfile + central fields without ZIP64 end records")
    previous = module(
        "previous_zip64",
        Path(__file__).resolve().parents[1] / "rust-archive-worker/hardening_cases.py",
    )
    for kind in ["locator64", "offset64-descriptor", "offset64-descriptor-unsigned"]:
        directory = root / kind
        directory.mkdir()
        previous.generate(directory, kind, 20)
        add("previous-" + kind, (directory / "input").read_bytes(), "PR74 regression", bytes(20))
    add("previous-eocd-disk", variant(original, 0, 3), "EOCD disk sentinel regression")
    add("previous-central-disk", variant(original, 8, 0), "central disk sentinel regression")
    (root / "cases.tsv").write_text(
        "".join("\t".join(row[:3]) + "\n" for row in cases), encoding="utf-8"
    )
    (root / "provenance.json").write_text(json.dumps({
        "python": sys.version,
        "libarchive": version,
        "infozip": subprocess.check_output(["zip", "-v"], text=True).splitlines()[:3],
        "cases": [{"id": row[0], "producer": row[3]} for row in cases],
    }, indent=4) + "\n", encoding="utf-8")
    print(f"Python reference CRC/content: {len(cases)} passed")


if __name__ == "__main__":
    generate(sys.argv[1])
