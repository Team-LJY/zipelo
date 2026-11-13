import bz2
import gzip
import lzma
from pathlib import Path
import sys
import zipfile


root = Path(sys.argv[1])
case = sys.argv[2]
if case == "bomb":
    with gzip.open(root / "input", "wb", compresslevel=9) as output:
        block = bytes(1024 * 1024)
        for _ in range(257):
            output.write(block)
elif case in {"header-budget-zip", "header-budget-tar"}:
    names = [f"{entry_id:04}-" + "x" * 45 for entry_id in range(1000)]
    if case.endswith("-zip"):
        with zipfile.ZipFile(root / "input", "w") as output:
            for name in names:
                output.writestr(name, b"")
    else:
        import tarfile

        with tarfile.open(root / "input", "w", format=tarfile.USTAR_FORMAT) as output:
            for name in names:
                output.addfile(tarfile.TarInfo(name))
elif case.startswith("header-budget-"):
    from seven_zip_fixtures import archive

    entries = [(f"{entry_id:04}-" + "x" * 45, b"", 0x20) for entry_id in range(1000)]
    size = int(case.rsplit("-", 1)[1])
    if size:
        entries.append(("payload", b"x" * size, 0x20))
    (root / "input").write_bytes(archive(entries, encoded="encoded" in case))
elif case.startswith("tar-stream-limit-"):
    import io
    import tarfile

    kind, mode = case.removeprefix("tar-stream-limit-").split("-")
    payload = bytes(index % 251 for index in range(65_537))
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        entry = tarfile.TarInfo("payload")
        entry.size = len(payload)
        archive.addfile(entry, io.BytesIO(payload))
    data = raw.getvalue()
    encode = {"gz": gzip.compress, "bz2": bz2.compress, "xz": lzma.compress}[kind]
    parts = (
        [data]
        if mode == "single"
        else [data[: len(data) // 2], data[len(data) // 2 :]]
    )
    (root / "input").write_bytes(b"".join(encode(part) for part in parts))
    (root / "expected").write_bytes(payload)
    (root / "stream-size").write_text(str(len(data)))
elif case == "wide-names":
    with zipfile.ZipFile(root / "input", "w") as archive:
        for entry_id in range(9000):
            archive.writestr(f"{entry_id:04}" + "x" * 236, b"")
elif case == "many-entries":
    with zipfile.ZipFile(root / "input", "w") as archive:
        for entry_id in range(200_000):
            archive.writestr(f"e{entry_id}", b"")
elif case in {"phase-directories", "phase-files"}:
    with zipfile.ZipFile(root / "input", "w") as archive:
        for entry_id in range(8):
            archive.writestr(
                f"entry-{entry_id}/" if case == "phase-directories" else f"entry-{entry_id}",
                b"" if case == "phase-directories" else b"x",
            )
elif case == "selection-entries":
    with zipfile.ZipFile(root / "input", "w") as archive:
        for entry_id in range(32_770):
            archive.writestr(f"e{entry_id}", b"x")
elif case == "empty-directories":
    with zipfile.ZipFile(root / "input", "w") as archive:
        for entry_id in range(36):
            archive.writestr(f"entry-{entry_id}/", b"")
elif case == "split-zip":
    with zipfile.ZipFile(root / "input", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        block = bytes(1024 * 1024)
        for name in ["first.bin", "second.bin"]:
            with archive.open(name, "w") as output:
                for _ in range(129):
                    output.write(block)
elif case in {"bz2", "xz"}:
    source = Path(__file__).with_name("fixtures").joinpath("sample.tar").read_bytes()
    encode = bz2.compress if case == "bz2" else lzma.compress
    (root / "input").write_bytes(encode(source))
else:
    raise ValueError(case)
