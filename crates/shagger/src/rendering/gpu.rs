//! The wgpu device. A window brings a surface; without one the device renders
//! offscreen (tests, screenshots, machines without a display).

/// Why no device could be made.
#[derive(Debug)]
pub struct GpuError(pub String);

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GpuError {}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// A device for `instance`, able to present to `surface` when given.
    ///
    /// # Errors
    /// When no adapter or device is available.
    pub fn new(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Self, GpuError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .map_err(|e| GpuError(format!("no GPU adapter: {e}")))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("shagger"),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| GpuError(format!("no GPU device: {e}")))?;
        // wgpu's default panics on any validation error; one bad resource
        // from a package should cost that resource, not the game.
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            eprintln!("gpu: {e}");
        }));
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// A device with no surface, for offscreen rendering.
    ///
    /// # Errors
    /// When no adapter or device is available.
    pub fn headless() -> Result<Self, GpuError> {
        Self::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        )
    }

    /// The adapter's name and backend, for logs.
    #[must_use]
    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?})", info.name, info.backend)
    }
}
