#!/usr/bin/env python3
"""Bundle Veloren's assets/ folder into one archive the Android app unpacks.

Format (matches voxygen/src/android.rs):
  b"VLRNASSETS1\n", then per file: u16 LE path length, UTF-8 path (relative,
  '/'-separated), u64 LE size, bytes. A zero path length ends the archive.
Also writes <out>.id: a content hash, so the app re-unpacks only when assets change.
"""
import hashlib, os, struct, sys

src, out = sys.argv[1], sys.argv[2]
h = hashlib.sha256()
files = 0
total = 0
with open(out, "wb") as f:
    f.write(b"VLRNASSETS1\n")
    for root, dirs, names in os.walk(src):
        dirs.sort()
        dirs[:] = [d for d in dirs if not d.startswith(".")]
        for name in sorted(names):
            if name.startswith("."):
                continue
            path = os.path.join(root, name)
            rel = os.path.relpath(path, src).replace(os.sep, "/")
            data = open(path, "rb").read()
            if data.startswith(b"version https://git-lfs.github.com/spec"):
                sys.exit(f"{rel} is a Git LFS pointer, not the real file")
            enc = rel.encode()
            f.write(struct.pack("<H", len(enc)) + enc + struct.pack("<Q", len(data)))
            f.write(data)
            h.update(enc); h.update(data)
            files += 1
            total += len(data)
    f.write(struct.pack("<H", 0))
with open(out.rsplit(".", 1)[0] + ".id", "w") as f:
    f.write(h.hexdigest()[:24] + "\n")
print(f"{files} files, {total/1e6:.0f} MB -> {out}")
