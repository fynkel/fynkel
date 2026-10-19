//! Getting a GPU device.

/// A wgpu device and its queue, with the instance and adapter they came
/// from (a target that draws to a window makes its surface with them). All
/// are cheap handles to clone.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub instance: wgpu::Instance,
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
        Self::from_adapter(instance, adapter)
    }

    /// A device on `adapter` (from `instance`), asking only for what the
    /// renderer needs.
    pub fn from_adapter(instance: wgpu::Instance, adapter: wgpu::Adapter) -> Result<Self, GpuError> {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fynkel"),
            // Buffers the CPU writes and the GPU reads in place, with no
            // staging copy (frame_ring.rs). Every native backend has it.
            // And a pipeline cache where there is one (Vulkan), kept
            // between runs (assets.rs). And drawing into 32-bit float
            // images where it can, for bloom's (bloom.rs). And sample
            // counts beyond WebGPU's 1 and 4 where it draws them, for
            // Quality::samples' 2.
            required_features: wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
                | (adapter.features()
                    & (wgpu::Features::PIPELINE_CACHE
                        | wgpu::Features::RG11B10UFLOAT_RENDERABLE
                        | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)),
            // WebGPU's defaults, lowered wherever this adapter offers less
            // (the iOS simulator sits below a few of them). The renderer
            // stays within the lower set.
            required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
            ..Default::default()
        }))
        .map_err(GpuError::NoDevice)?;
        Ok(Self { instance, adapter, device, queue })
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
