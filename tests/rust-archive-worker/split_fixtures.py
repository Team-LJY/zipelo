"""독립 7-Zip으로 분할 표본을 만들고 해제 해시를 대조함."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--tool", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    tool = args.tool.resolve()
    work = args.work.resolve()
    output = args.output.resolve()
    work.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)
    assert not any(work.iterdir()) and not any(output.iterdir())
    source = work / "source"
    (source / "folder").mkdir(parents=True)
    expected = {
        "folder/hello.txt": b"Zipelo 7z interoperability\n" * 100,
        "second.bin": bytes((index * 37 + (index >> 3)) % 256 for index in range(4096)),
    }
    for name, data in expected.items():
        (source / name).write_bytes(data)

    def run(arguments, cwd):
        return subprocess.check_output([str(tool), *arguments], cwd=cwd, text=True)

    version = run([], source).splitlines()[1]
    fixtures = {}
    variants = [
        ("reference", []),
        ("encrypted-header", ["-pzipelo-split-test", "-mhe=on"]),
        ("encrypted-body", ["-pzipelo-split-test", "-mhe=off"]),
    ]
    for name, encryption in variants:
        options = [
            "a", "-t7z", "-m0=LZMA2", "-md=1m", "-ms=on", "-mhc=off",
            "-mta=off", "-mtc=off", "-mtm=off", "-v64b", *encryption,
        ]
        run([*options, str(output / f"{name}.7z"), "folder", "second.bin"], source)
        parts = sorted(output.glob(f"{name}.7z.[0-9][0-9][0-9]"))
        assert len(parts) > 1
        extracted = work / name
        run(
            ["x", "-y", "-pzipelo-split-test", f"-o{extracted}", str(parts[0])],
            source,
        )
        actual = {
            entry.relative_to(extracted).as_posix(): entry.read_bytes()
            for entry in extracted.rglob("*") if entry.is_file()
        }
        assert actual == expected
        digest = hashlib.sha256()
        for part in parts:
            digest.update(part.read_bytes())
        fixtures[name] = {
            "arguments": options,
            "parts": [
                {
                    "name": part.name,
                    "bytes": part.stat().st_size,
                    "sha256": hashlib.sha256(part.read_bytes()).hexdigest(),
                }
                for part in parts
            ],
            "logical_sha256": digest.hexdigest(),
        }
    manifest = {
        "tool": version,
        "source": "Ubuntu 7zip 26.00+dfsg-1",
        "payload_sha256": {
            name: hashlib.sha256(data).hexdigest() for name, data in expected.items()
        },
        "fixtures": fixtures,
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=4) + "\n", encoding="utf-8"
    )
    print(json.dumps({
        "tool": version,
        "parts": {name: len(data["parts"]) for name, data in fixtures.items()},
    }, ensure_ascii=False))


if __name__ == "__main__":
    main()
