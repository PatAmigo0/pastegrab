"""Download Windows x64 wheels for CPython 3.11 from PyPI, verified against PyPI's SHA-256.

Usage: python3 -I wheels.py OUT_DIR name==version ...
"""
import hashlib
import json
import re
import sys
import urllib.request


def pick(files):
    def rank(name):
        if name.endswith("-cp311-cp311-win_amd64.whl"):
            return 0
        m = re.search(r"-cp3(\d+)-abi3-win_amd64\.whl$", name)
        if m and int(m.group(1)) <= 11:
            return 1
        if name.endswith("-py3-none-any.whl") or name.endswith("-py2.py3-none-any.whl"):
            return 2
        return None

    ranked = [(rank(f["filename"]), f) for f in files if rank(f["filename"]) is not None]
    if not ranked:
        return None
    return min(ranked, key=lambda r: r[0])[1]


def main():
    out = sys.argv[1]
    for spec in sys.argv[2:]:
        name, version = spec.split("==")
        with urllib.request.urlopen(f"https://pypi.org/pypi/{name}/{version}/json") as r:
            files = json.load(r)["urls"]
        f = pick(files)
        if f is None:
            sys.exit(f"no cp311 win_amd64 wheel for {spec}")
        with urllib.request.urlopen(f["url"]) as r:
            data = r.read()
        if hashlib.sha256(data).hexdigest() != f["digests"]["sha256"]:
            sys.exit(f"hash mismatch for {f['filename']}")
        with open(f"{out}/{f['filename']}", "wb") as w:
            w.write(data)
        print(f["filename"])


main()
