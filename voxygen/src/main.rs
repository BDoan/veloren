#![deny(unsafe_code)]
#![recursion_limit = "2048"]

#[cfg(all(
    target_os = "windows",
    not(feature = "tracy-memory"),
    not(feature = "hot-egui"),
    not(feature = "hot-anim"),
))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Allow profiling allocations with Tracy
#[cfg_attr(feature = "tracy-memory", global_allocator)]
#[cfg(feature = "tracy-memory")]
static GLOBAL: common_base::tracy_client::ProfiledAllocator<std::alloc::System> =
    common_base::tracy_client::ProfiledAllocator::new(std::alloc::System, 128);

use veloren_voxygen::cli;
use wgpu::{Backends, Instance};

fn main() {
    // Process CLI arguments
    use clap::Parser;
    let args = cli::Args::parse();

    if let Some(command) = args.command {
        match command {
            cli::Commands::ListWgpuBackends => {
                #[cfg(target_os = "windows")]
                let backends = &["opengl", "dx12", "vulkan"];
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                let backends = &["opengl", "vulkan"];
                #[cfg(target_os = "macos")]
                let backends = &["metal"];

                for backend in backends {
                    println!("{backend}");
                }
                return;
            },
            cli::Commands::ListWgpuDevices => {
                let adapters = Instance::new(&wgpu::InstanceDescriptor::from_env_or_default())
                    .enumerate_adapters(Backends::default());
                for adapter in adapters {
                    println!("{}", adapter.get_info().name);
                }
                return;
            },
        }
    }

    veloren_voxygen::start::start(args);
}
