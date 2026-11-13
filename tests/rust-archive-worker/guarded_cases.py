import bz2
import gzip
import lzma
from pathlib import Path
import sys


root = Path(sys.argv[1])
case = sys.argv[2]
if case == "bomb":
    with gzip.open(root / "input", "wb", compresslevel=9) as output:
        block = bytes(1024 * 1024)
        for _ in range(257):
            output.write(block)
elif case in {"bz2", "xz"}:
    source = Path(__file__).with_name("fixtures").joinpath("sample.tar").read_bytes()
    encode = bz2.compress if case == "bz2" else lzma.compress
    (root / "input").write_bytes(encode(source))
else:
    raise ValueError(case)
