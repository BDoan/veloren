//! Android entry point for Voxygen.
//!
//! `android-activity` (used by winit's NativeActivity backend) calls
//! `android_main` on a dedicated native thread once the activity starts.

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: voxygen::android::AndroidApp) { voxygen::android::main(app); }
