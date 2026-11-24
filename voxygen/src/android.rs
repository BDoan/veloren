//! Android platform glue.
//!
//! * Routes stdout/stderr (where tracing and panic output go) to logcat.
//! * Points Veloren's userdata at app-specific external storage so settings,
//!   saves and logs can be reached over USB.
//! * Unpacks the game assets bundled in the APK on first launch (and after an
//!   update), since Veloren reads assets from a normal directory.
//! * Holds the `AndroidApp` handle that winit needs to build its event loop.
#![allow(unsafe_code)]

use std::{
    ffi::CString,
    fs,
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::pump_events::{EventLoopExtPumpEvents, PumpStatus},
    window::WindowId,
};

pub use winit::platform::android::activity::AndroidApp;
use winit::platform::android::activity::ndk_sys;

static APP: OnceLock<AndroidApp> = OnceLock::new();

const ASSET_ARCHIVE: &str = "veloren-assets.bin";
const ASSET_ARCHIVE_ID: &str = "veloren-assets.id";
const ARCHIVE_MAGIC: &[u8; 12] = b"VLRNASSETS1\n";
const LOG_TAG: &str = "veloren";

/// The running app. Only valid after [`main`] has started.
pub fn app() -> &'static AndroidApp { APP.get().expect("AndroidApp not initialised") }

/// Entry point, called from `android_main` in the `veloren-voxygen-android`
/// crate.
pub fn main(app: AndroidApp) {
    redirect_stdio_to_logcat();
    log_line(&format!(
        "Veloren starting (voxygen {})",
        env!("CARGO_PKG_VERSION")
    ));

    let internal = app
        .internal_data_path()
        .expect("Android did not provide an internal data path");
    let external = app.external_data_path();

    // Userdata: settings, singleplayer worlds, screenshots and logs.
    let userdata = external.unwrap_or_else(|| internal.clone()).join("userdata");
    if let Err(e) = fs::create_dir_all(&userdata) {
        log_line(&format!("Could not create userdata dir {}: {e}", userdata.display()));
    }

    let assets = match unpack_assets(&app, &internal) {
        Ok(path) => path,
        Err(e) => {
            log_line(&format!("FATAL: could not unpack game assets: {e}"));
            panic!("Could not unpack game assets: {e}");
        },
    };

    // SAFETY: we are still the only Rust thread touching the environment;
    // nothing else has been spawned yet.
    unsafe {
        std::env::set_var("VELOREN_USERDATA", &userdata);
        std::env::set_var("VELOREN_ASSETS", &assets);
        // Some code looks up the home and pictures folders, which Android
        // does not define.
        std::env::set_var("HOME", &userdata);
        std::env::set_var("VOXYGEN_SCREENSHOT", userdata.join("screenshots"));
        std::env::set_var("RUST_BACKTRACE", "1");
        // Vulkan only: the GLES path is untested and much slower.
        std::env::set_var("WGPU_BACKEND", "vulkan");
    }

    let _ = APP.set(app);

    use clap::Parser;
    let args = crate::cli::Args::parse_from(["veloren-voxygen"]);
    crate::start::start(args);

    log_line("Veloren exited");
    // NativeActivity keeps the process alive after `android_main` returns,
    // and a second launch would find statics already initialised.
    std::process::exit(0);
}

/// Phone-friendly defaults, applied only when no settings file exists yet.
pub fn apply_default_settings(settings: &mut crate::settings::Settings) {
    use crate::ui::ScaleMode;
    // Low preset: phones have far less GPU headroom than desktops, and the
    // inner screen of a foldable is still ~4 megapixels.
    let graphics = std::mem::take(&mut settings.graphics);
    settings.graphics = graphics.into_low();
    settings.graphics.max_fps = crate::settings::Fps::Max(60);
    settings.graphics.max_background_fps = crate::settings::Fps::Max(10);
    // The HUD is laid out for a 1080p monitor; on a phone that makes text
    // tiny. Lay it out for a smaller virtual screen so everything is bigger.
    settings.interface.ui_scale = ScaleMode::RelativeToWindow([1600.0, 900.0].into());
}

/// Block until Android gives us a native window to draw into. winit only
/// reports it through the `Resumed` event, so pump the loop until then.
pub fn wait_for_native_window(event_loop: &mut EventLoop<()>) {
    struct Waiter {
        resumed: bool,
    }
    impl ApplicationHandler for Waiter {
        fn resumed(&mut self, _: &ActiveEventLoop) { self.resumed = true; }

        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }

    let mut waiter = Waiter { resumed: false };
    while !waiter.resumed {
        if let PumpStatus::Exit(_) =
            event_loop.pump_app_events(Some(Duration::from_millis(100)), &mut waiter)
        {
            log_line("Activity closed before a window was created");
            std::process::exit(0);
        }
    }
}

/// Write one line to logcat directly (works before stdio is redirected).
pub fn log_line(msg: &str) {
    let tag = CString::new(LOG_TAG).unwrap();
    let text = CString::new(msg.replace('\0', "")).unwrap_or_default();
    unsafe {
        ndk_sys::__android_log_write(
            ndk_sys::android_LogPriority::ANDROID_LOG_INFO.0 as _,
            tag.as_ptr(),
            text.as_ptr(),
        );
    }
}

/// Android discards stdout/stderr. Pipe them into logcat on a helper thread so
/// tracing output and panic messages show up in `adb logcat -s veloren`.
fn redirect_stdio_to_logcat() {
    let mut fds = [0 as libc::c_int; 2];
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(fds[1], libc::STDOUT_FILENO);
        libc::dup2(fds[1], libc::STDERR_FILENO);
        libc::close(fds[1]);
    }
    let read_fd = fds[0];
    let _ = std::thread::Builder::new()
        .name("logcat".into())
        .spawn(move || {
            use std::os::fd::FromRawFd;
            let file = unsafe { fs::File::from_raw_fd(read_fd) };
            let reader = io::BufReader::new(file);
            for line in io::BufRead::lines(reader) {
                match line {
                    Ok(line) => log_line(&line),
                    Err(_) => break,
                }
            }
        });
}

fn read_apk_asset(app: &AndroidApp, name: &str) -> io::Result<ndk_asset::Asset> {
    let cname = CString::new(name).unwrap();
    app.asset_manager().open(&cname).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("{name} missing from APK"))
    })
}

use winit::platform::android::activity::ndk::asset as ndk_asset;

/// Unpack `veloren-assets.bin` from the APK into `<internal>/assets` unless the
/// same build's assets are already there. Returns the assets directory.
fn unpack_assets(app: &AndroidApp, internal: &Path) -> io::Result<PathBuf> {
    let dest = internal.join("assets");
    let marker = internal.join("assets.id");

    let mut wanted_id = String::new();
    read_apk_asset(app, ASSET_ARCHIVE_ID)?.read_to_string(&mut wanted_id)?;
    let wanted_id = wanted_id.trim().to_owned();

    if dest.is_dir() && fs::read_to_string(&marker).ok().as_deref().map(str::trim) == Some(wanted_id.as_str())
    {
        log_line(&format!("Assets up to date ({wanted_id})"));
        return Ok(dest);
    }

    log_line(&format!("Unpacking assets {wanted_id} to {}", dest.display()));
    let started = std::time::Instant::now();
    let _ = fs::remove_file(&marker);
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    fs::create_dir_all(&dest)?;

    let mut reader = BufReader::with_capacity(1 << 20, read_apk_asset(app, ASSET_ARCHIVE)?);
    let mut magic = [0u8; 12];
    reader.read_exact(&mut magic)?;
    if &magic != ARCHIVE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad asset archive header"));
    }

    let mut files = 0usize;
    let mut bytes = 0u64;
    loop {
        let mut len = [0u8; 2];
        reader.read_exact(&mut len)?;
        let len = u16::from_le_bytes(len) as usize;
        if len == 0 {
            break;
        }
        let mut name = vec![0u8; len];
        reader.read_exact(&mut name)?;
        let name = String::from_utf8(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 asset path"))?;
        let mut size = [0u8; 8];
        reader.read_exact(&mut size)?;
        let size = u64::from_le_bytes(size);

        // Reject anything that could escape the destination directory.
        let rel = Path::new(&name);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsafe asset path {name}"),
            ));
        }
        let path = dest.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = BufWriter::with_capacity(1 << 20, fs::File::create(&path)?);
        let copied = io::copy(&mut (&mut reader).take(size), &mut out)?;
        if copied != size {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated asset archive"));
        }
        out.flush()?;
        files += 1;
        bytes += size;
    }

    fs::write(&marker, &wanted_id)?;
    log_line(&format!(
        "Unpacked {files} asset files ({} MB) in {:.1}s",
        bytes / 1_000_000,
        started.elapsed().as_secs_f32()
    ));
    Ok(dest)
}
