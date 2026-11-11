import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

HERE = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inventory(data):
    require(data.get("truncated") is False, "잘린 tree 거부")
    rows = [e for e in data["tree"] if e["type"] != "tree"]
    require(len(rows) == len({e["path"] for e in rows}), "tree 중복 경로")
    return {e["path"]: (e["mode"], e["type"], e["sha"]) for e in rows}


def main():
    require(not sys.flags.optimize, "최적화 모드 거부")
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--base-tree", type=Path, required=True)
    parser.add_argument("--input-tree", type=Path, required=True)
    args = parser.parse_args()
    manifest = (HERE / "SHA256SUMS").read_bytes()
    require(hashlib.sha256(manifest).hexdigest() == args.manifest_sha256, "manifest hash 불일치")
    names = set()
    for line in manifest.decode("ascii").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([a-z_./-]+)", line)
        require(match is not None, "manifest 행 형식")
        digest, name = match.groups()
        require(name in {"overlay.json", "run-plan.json", "static_check.py"} and name not in names, "manifest 경로")
        names.add(name)
        require(hashlib.sha256((HERE / name).read_bytes()).hexdigest() == digest, "metadata hash 불일치")
    require(names == {"overlay.json", "run-plan.json", "static_check.py"}, "manifest 누락")
    overlay = json.loads((HERE / "overlay.json").read_text())
    plan = json.loads((HERE / "run-plan.json").read_text())
    base_data = json.loads(args.base_tree.read_text())
    input_data = json.loads(args.input_tree.read_text())
    require(base_data["sha"] == overlay["base"]["tree"], "base tree 불일치")
    require(input_data["sha"] == overlay["input_tree"], "입력 tree 불일치")
    base, current = inventory(base_data), inventory(input_data)
    expected = {e["path"]: (e["mode"], e["type"], e["sha"]) for e in overlay["entries"]}
    require(len(expected) == len(overlay["entries"]) == 376, "입력 경로 중복/개수")
    require(expected == current and len(base) == 305, "입력 inventory 불일치")
    require(not set(base) - set(current), "기존 경로 삭제")
    replaced = {p for p in base if base[p] != current[p]}
    allowed = {e["path"] for e in overlay["replacements"]}
    require(replaced == allowed and len(replaced) == 3, "교체 범위 오류")
    for item in overlay["replacements"]:
        for key, source in [("before", base), ("after", current)]:
            row = item[key]
            require(source[item["path"]] == (row["mode"], "blob", row["blob"]), "교체 blob/mode 불일치")
            require(re.fullmatch(r"[0-9a-f]{64}", row["sha256"]) is not None, "교체 SHA256 형식")
    require(len(set(current) - set(base)) == 71, "추가 개수 오류")
    require(all(base[p] == current[p] for p in base if p.endswith(".cs")), "C# 변경")
    require({p["pr"] for p in overlay["inputs"]} == {54, 55, 56}, "입력 PR 범위")
    require(not any("worker-session" in p for p in current), "새 worker-session 반입")
    ids = []
    for group in plan["groups"]:
        require(group["count"] == len(group["exact_names"]) == len(set(group["exact_names"])), "그룹 중복/개수")
        require(group["manifest"] in current, "Cargo manifest 누락")
        require(group["count"] - 1 == group["filtered_per_case"], "filtered 계약")
        ids.extend((group["manifest"], group["target"], name) for name in group["exact_names"])
    require(len(ids) == len(set(ids)) == 807 and len(plan["groups"]) == 11, "807개 고유 집합 오류")
    print(json.dumps({"status": "passed", "input_files": 376, "added": 71, "replaced": 3,
                      "preserved_base": 302, "groups": 11, "planned_unique_cases": 807,
                      "rust_compiled": False, "executed_cases": 0}))


if __name__ == "__main__":
    main()
