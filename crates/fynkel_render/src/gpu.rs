//! Getting a GPU device.

/// A wgpu device and its queue. Both are cheap handles to clone.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Why no GPU could be had.
#[derive(Debug)]
pub enum GpuError {
    NoAdapter(wgpu::RequestAdapterError),
    NoDevice(wgpu::RequestDeviceError),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuError::NoAdapter(e) => write!(f, "no GPU adapter: {e}"),
            GpuError::NoDevice(e) => write!(f, "the GPU refused a device: {e}"),
        }
    }
}

impl std::error::Error for GpuError {}

impl Gpu {
    /// A device with no window or surface, on the platform's main API
    /// (Metal on Apple, Vulkan on Android). Used by tests and the headless
    /// target.
    pub fn headless() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            flags: instance_flags(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(GpuError::NoAdapter)?;
        Self::from_adapter(adapter)
    }

    /// A device on `adapter`, asking only for what the renderer needs.
    pub fn from_adapter(adapter: wgpu::Adapter) -> Result<Self, GpuError> {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fynkel"),
            required_features: wgpu::Features::empty(),
            // WebGPU's defaults, lowered wherever this adapter offers less
            // (the iOS simulator sits below a few of them). The renderer
            // stays within the lower set.
            required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
            ..Default::default()
        }))
        .map_err(GpuError::NoDevice)?;
        Ok(Self { adapter, device, queue })
    }
}

/// wgpu's defaults for this build, plus one Android fix: the Android
/// emulator's Vulkan driver (`vulkan.ranchu.so`, over MoltenVK) crashes in
/// `vkSetDebugUtilsObjectNameEXT` when wgpu names its objects in debug
/// builds. Not passing names to the driver avoids it, and costs nothing but
/// names in GPU debuggers. Apple keeps them for Xcode's frame capture.
pub fn instance_flags() -> wgpu::InstanceFlags {
    let flags = wgpu::InstanceFlags::from_build_config();
    if cfg!(target_os = "android") { flags | wgpu::InstanceFlags::DISCARD_HAL_LABELS } else { flags }
}
