#!/bin/bash
# Turn the compiled native library into a signed, sideloadable APK.
#
# Env: ANDROID_HOME (SDK with build-tools 35.0.0 and platforms;android-35),
#      ANDROID_NDK_HOME, optional VERSION_CODE / VERSION_NAME, and optional
#      ANDROID_KEYSTORE / ANDROID_KEYSTORE_PASSWORD / ANDROID_KEY_ALIAS
#      (defaults to the debug keystore in android/).
set -euo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)
PKG=$REPO/android
: "${ANDROID_HOME:?set ANDROID_HOME}"
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME}"
BT=$ANDROID_HOME/build-tools/35.0.0
PLAT=$ANDROID_HOME/platforms/android-35/android.jar
NDK_BIN=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
SO_IN=$REPO/target/aarch64-linux-android/android/libveloren_voxygen_android.so
VERSION_CODE=${VERSION_CODE:-1}
VERSION_NAME=${VERSION_NAME:-0.18.0-android.$VERSION_CODE}
KEYSTORE=${ANDROID_KEYSTORE:-$PKG/veloren-debug.keystore}
KEY_PASS=${ANDROID_KEYSTORE_PASSWORD:-android}
KEY_ALIAS=${ANDROID_KEY_ALIAS:-veloren}
OUTDIR=$REPO/target/apk
B=$OUTDIR/build
rm -rf "$B"; mkdir -p "$B/classes" "$B/dex"

echo "== resources"
"$BT/aapt2" compile --dir "$PKG/res" -o "$B/res.zip"
"$BT/aapt2" link -o "$B/base.apk" -I "$PLAT" --manifest "$PKG/AndroidManifest.xml" \
  --min-sdk-version 30 --target-sdk-version 35 \
  --version-code "$VERSION_CODE" --version-name "$VERSION_NAME" "$B/res.zip"

echo "== java"
javac -source 11 -target 11 -Xlint:-options -nowarn -classpath "$PLAT" -d "$B/classes" \
  "$PKG"/java/net/veloren/android/*.java
"$BT/d8" --release --min-api 30 --lib "$PLAT" --output "$B/dex" \
  $(find "$B/classes" -name '*.class')

echo "== native library"
cp "$SO_IN" "$B/lib.so"
"$NDK_BIN/llvm-strip" --strip-debug "$B/lib.so"
"$NDK_BIN/llvm-nm" -D --defined-only "$B/lib.so" | grep -q " ANativeActivity_onCreate$" \
  || { echo "ANativeActivity_onCreate is not exported"; exit 1; }

echo "== assets"
python3 -I "$PKG/scripts/make_assets.py" "$REPO/assets" "$OUTDIR/veloren-assets.bin"

echo "== assemble, align, sign"
python3 -I "$PKG/scripts/assemble.py" "$B/base.apk" "$B/dex/classes.dex" "$B/lib.so" \
  "$OUTDIR/veloren-assets.bin" "$OUTDIR/veloren-assets.id" "$B/unsigned.apk"
"$BT/zipalign" -P 16 -f 4 "$B/unsigned.apk" "$B/aligned.apk"
OUT="$OUTDIR/veloren-android-$VERSION_NAME.apk"
rm -f "$OUTDIR"/veloren-android-*.apk
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-pass "pass:$KEY_PASS" --ks-key-alias "$KEY_ALIAS" \
  --out "$OUT" "$B/aligned.apk"
"$BT/apksigner" verify "$OUT"
"$BT/aapt2" dump badging "$OUT" | head -3
ls -la "$OUT"
