# Veloren for Android (personal port)

An unofficial port of the Veloren client to Android, built for a Samsung
Galaxy Z Fold 7. It runs single player using the game's built-in server, with
touchscreen controls. It is not affiliated with the Veloren project.

## Getting the APK

Every push to the `android` branch builds the APK with GitHub Actions
(`.github/workflows/android-apk.yml`) and publishes it under **Releases**
(the newest 3 builds are kept). On the phone: open the latest release, download
the `.apk`, open it and allow installs from that app when asked.

The first launch unpacks about 450 MB of game data, so the screen stays black
for a few seconds.

## Controls

**In game**
- **Left side:** drag anywhere to move. The joystick appears under your thumb.
- **Right side:** drag to look around; pinch to zoom.
- **Big ATK button:** primary attack. Drag while holding ATK, ALT or BLOCK to aim.
- **Ring around it:** ALT (secondary attack), BLOCK, ROLL and JUMP.
- **Outer ring:** USE (interact, talk, pick up), GLIDE and SNEAK. The 1–5 buttons are ability slots.
- **Top-left row:** MENU, BAG, MAP, WIELD, SWAP (loadout), SIT, MOUNT and LIGHT.
- **RESPAWN** appears when you die.

**Menus and open windows** (inventory, map and so on)
- Tap to click.
- Drag to drag, for example to move items.
- Long-press to right-click, which equips or uses items in the inventory.
- Use a second finger to scroll.
- **BACK** (top-left) closes windows and returns to the game.
- The phone's back gesture works like Escape.

## Where files go

- **Settings and worlds:** the app's private storage. They are deleted when you uninstall.
- **Logs and screenshots:** `Android/data/net.veloren.android/files/`, which you can reach from a PC over USB.
- **Crashes:** if the game crashes, the error is shown as a popup the next time it starts, so you can screenshot it.
- **Live log:** `adb logcat -s veloren`

## Building locally

Requirements: the Rust toolchain from `rust-toolchain` with the
`aarch64-linux-android` target, `cargo install cargo-ndk`, and an Android SDK
with NDK r27 (`ndk;27.2.12479018`), `build-tools;35.0.0` and `platforms;android-35`.

```sh
export ANDROID_HOME=~/android-sdk ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
android/build.sh               # -> target/aarch64-linux-android/android/*.so
android/scripts/package.sh     # -> target/apk/veloren-android-*.apk
```

## Signing

APKs are signed with `android/veloren-debug.keystore` (password `android`).
Android only installs an update over an existing install if both are signed
with the same key, so keep using this key.

To use a private key instead, add two repository secrets:
- `ANDROID_KEYSTORE_B64`: the base64-encoded keystore, with alias `veloren`.
- `ANDROID_KEYSTORE_PASSWORD`: its password.

You will have to uninstall once when you switch keys, which deletes your worlds.

## What changed from upstream

- `voxygen/android/`: the `cdylib` entry point (`android_main`).
- `voxygen/src/android.rs`:
  - logcat output
  - storage paths
  - unpacking assets from the APK
  - waiting for the native window
  - phone-friendly first-run settings
- `voxygen/src/touch.rs` and the overlay at the end of `Hud::update_layout`: touch controls.
- Renderer: the surface is dropped and recreated when the app is backgrounded. It prefers an sRGB swapchain and picks a supported alpha mode. `DEPTH_CLIP_CONTROL` is optional.
- shaderc and Mumble are not built on Android; shaders compile through naga, which is Veloren's default.
- `voxygen/src/start.rs`: the shared start-up code, moved out of `main.rs`.
