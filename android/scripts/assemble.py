#!/usr/bin/env python3
"""Combine the aapt2 base APK, classes.dex, the native library and assets."""
import sys, zipfile

base, dex, so, assets_bin, assets_id, out = sys.argv[1:7]
with zipfile.ZipFile(base) as zin, zipfile.ZipFile(out, "w") as zout:
    for info in zin.infolist():
        # Keep aapt2's choices (resources.arsc must stay uncompressed).
        zout.writestr(info, zin.read(info.filename))
    zout.write(dex, "classes.dex", compress_type=zipfile.ZIP_DEFLATED)
    zout.write(so, "lib/arm64-v8a/libveloren_voxygen_android.so",
               compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    zout.write(assets_id, "assets/veloren-assets.id", compress_type=zipfile.ZIP_STORED)
    zout.write(assets_bin, "assets/veloren-assets.bin",
               compress_type=zipfile.ZIP_DEFLATED, compresslevel=6)
print("assembled", out)
