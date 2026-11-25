//! OpenXR runtime (Vulkan on Linux, D3D12 on Windows) and VR camera helpers.
//!
//! Linux PC VR runtimes for Meta Quest 2 (WiVRn, Monado, SteamVR, ALVR) expose
//! OpenXR over Vulkan (`XR_KHR_vulkan_enable2` or `XR_KHR_vulkan_enable`).
//! Because those runtimes require specific Vulkan instance/device extensions
//! (e.g. DMA-BUF / external memory & semaphore sharing) and select the GPU
//! driving the headset, [`XrVulkanBootstrap`] creates the `wgpu::Instance`,
//! `wgpu::Adapter`, and `wgpu::Device` through `wgpu-hal` before opening the
//! OpenXR session.

use glam::{DQuat, DVec3, Mat4, Quat, Vec3};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether an OpenXR VR session is currently active.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn set_active(on: bool) {
    ACTIVE.store(on, Ordering::Relaxed);
}

/// Base world pose of the seated cockpit head for the current frame, passed
/// into [`crate::render::Renderer::render`] so per-eye OpenXR poses can be
/// applied cleanly without inverting jittered projection matrices.
#[derive(Copy, Clone, Debug)]
pub struct XrViewBase {
    /// World orientation of the reference frame (aircraft cockpit attitude in
    /// cockpit view, or the base camera orientation in menu/external views).
    pub cockpit_rot: Quat,
    /// Recentered head position (body frame, m) that was already baked into
    /// the frame's camera world origin `cam`. Subtracted from each eye's
    /// predicted position so only the tiny IPD + inter-frame delta remains.
    pub head_pos: Vec3,
}

/// Normalizes a quaternion safely, falling back to `fallback` if zero/non-finite.
fn safe_quat(q: Quat, fallback: Quat) -> Quat {
    let l2 = q.length_squared();
    if l2.is_finite() && l2 > 1e-12 {
        q / l2.sqrt()
    } else {
        fallback
    }
}

/// Extracts only the horizontal yaw rotation around +Y from a right-handed
/// Y-up, -Z-forward orientation quaternion. Used when capturing the seated
/// VR recenter origin so tilting the head never tilts the horizon.
pub fn yaw_only(q: Quat) -> Quat {
    let q = safe_quat(q, Quat::IDENTITY);
    let fwd = q * Vec3::NEG_Z;
    if fwd.x.hypot(fwd.z) > 1e-3 {
        Quat::from_rotation_y((-fwd.x).atan2(-fwd.z))
    } else {
        Quat::IDENTITY
    }
}

/// Infinite far plane, reverse-Z (1 at `near`, 0 at infinity) asymmetric
/// perspective matrix for an OpenXR eye FOV, matching wgpu's clip space
/// (+X right, +Y up, Z in [0, 1]).
pub fn asymmetric_projection_rev(fov: openxr::Fovf, near: f32) -> Mat4 {
    let tan_l = fov.angle_left.tan();
    let tan_r = fov.angle_right.tan();
    let tan_d = fov.angle_down.tan();
    let tan_u = fov.angle_up.tan();
    let n = near.max(0.01);
    let inv_w = 1.0 / (tan_r - tan_l).max(1e-4);
    let inv_h = 1.0 / (tan_u - tan_d).max(1e-4);
    Mat4::from_cols_array(&[
        2.0 * inv_w, 0.0, 0.0, 0.0,
        0.0, 2.0 * inv_h, 0.0, 0.0,
        (tan_r + tan_l) * inv_w, (tan_u + tan_d) * inv_h, 0.0, -1.0,
        0.0, 0.0, n, 0.0,
    ])
}

/// Builds `(view_proj, inv_view_proj)` for one OpenXR eye.
///
/// `view_proj` includes the eye's translation relative to the frame's camera
/// origin (`cam`) for exact stereoscopic parallax on the cockpit and world.
/// `inv_view_proj` intentionally omits translation so `view_ray(uv)` in
/// `common.wgsl`, `composite.wgsl`, `clouds.wgsl`, and `ao.wgsl` returns a
/// pure ray from the eye rather than amplifying the IPD offset by `1 / near`.
pub fn eye_matrices(
    base: XrViewBase,
    eye_pos_rel: Vec3,
    eye_rot_rel: Quat,
    fov: openxr::Fovf,
    near: f32,
) -> (Mat4, Mat4) {
    let p = asymmetric_projection_rev(fov, near);
    let eye_offset_world = base.cockpit_rot * (eye_pos_rel - base.head_pos);
    let eye_world_rot = safe_quat(base.cockpit_rot * eye_rot_rel, base.cockpit_rot);
    let view_rot = Mat4::from_quat(eye_world_rot.conjugate());
    let view = view_rot * Mat4::from_translation(-eye_offset_world);
    let vp = p * view;
    let inv_vp = (p * view_rot).inverse();
    (vp, inv_vp)
}

#[cfg(target_os = "linux")]
type XrGfx = openxr::Vulkan;
#[cfg(windows)]
type XrGfx = openxr::D3D12;

#[cfg(any(windows, target_os = "linux"))]
#[derive(Clone)]
pub struct XrFrame {
    pub display_time: openxr::Time,
    /// Recentered per-eye poses & FOVs (in cockpit body frame: +X right, +Y up, -Z forward).
    pub views: Vec<openxr::View>,
    pub should_render: bool,
}

#[cfg(any(windows, target_os = "linux"))]
struct ActiveXrFrame {
    display_time: openxr::Time,
    /// Raw OpenXR views in `space` (submitted unchanged to `xrEndFrame` for ATW/ASW).
    views: Vec<openxr::View>,
    images: [Option<usize>; 2],
    should_render: bool,
}

/// Active OpenXR headset session.
///
/// Field drop order is critical: `views` and `textures` MUST drop before
/// `swapchains`, and `swapchains` / `space` / `frame_stream` / `frame_wait` /
/// `session` MUST drop before `instance` and `_entry`.
#[cfg(any(windows, target_os = "linux"))]
pub struct OpenXrRuntime {
    views: [Vec<wgpu::TextureView>; 2],
    _textures: [Vec<wgpu::Texture>; 2],
    swapchains: [openxr::Swapchain<XrGfx>; 2],
    space: openxr::Space,
    frame_stream: openxr::FrameStream<XrGfx>,
    frame_wait: openxr::FrameWaiter,
    session: openxr::Session<XrGfx>,
    _system: openxr::SystemId,
    instance: openxr::Instance,
    _entry: openxr::Entry,
    extent: [u32; 2],
    eye_extents: [[u32; 2]; 2],
    format: wgpu::TextureFormat,
    blend: openxr::EnvironmentBlendMode,
    runtime_name: String,
    session_state: openxr::SessionState,
    running: bool,
    exiting: bool,
    active_frame: Option<ActiveXrFrame>,
    tracking_origin: Option<(Quat, Vec3)>,
    last_head_pose: Option<(Vec3, Quat)>,
}

/// Linux Vulkan bootstrap: creates the OpenXR instance, queries the runtime's
/// Vulkan requirements, creates the `VkInstance` / `VkPhysicalDevice` /
/// `VkDevice` via `XR_KHR_vulkan_enable2` (or `XR_KHR_vulkan_enable`), and
/// wraps them for `wgpu`.
#[cfg(target_os = "linux")]
pub struct XrVulkanBootstrap {
    entry: openxr::Entry,
    instance: openxr::Instance,
    system: openxr::SystemId,
    use_enable2: bool,
    vk_entry: ash::Entry,
    vk_instance: ash::Instance,
    vk_physical_device: ash::vk::PhysicalDevice,
}

#[cfg(target_os = "linux")]
impl XrVulkanBootstrap {
    /// Connects to the active Linux OpenXR runtime and builds a compatible
    /// `wgpu::Instance` and `wgpu::Adapter` on the headset's GPU.
    pub fn init() -> anyhow::Result<(wgpu::Instance, wgpu::Adapter, Self)> {
        use ash::vk::{self, Handle as _};
        use std::ffi::{CStr, CString, c_void};

        let entry = unsafe { openxr::Entry::load(&()) }
            .or_else(|e1| unsafe { openxr::Entry::load_from(Path::new("libopenxr_loader.so"), &()) }.map_err(|_| e1))
            .map_err(|e| anyhow::anyhow!("OpenXR loader (libopenxr_loader.so.1) unavailable: {e}"))?;
        let available = entry
            .enumerate_extensions()
            .map_err(|e| anyhow::anyhow!("could not query OpenXR extensions (is an OpenXR runtime such as WiVRn, SteamVR, or Monado active?): {e}"))?;
        let use_enable2 = if available.khr_vulkan_enable2 {
            true
        } else if available.khr_vulkan_enable {
            false
        } else {
            anyhow::bail!("the active OpenXR runtime does not support Vulkan (neither XR_KHR_vulkan_enable2 nor XR_KHR_vulkan_enable)");
        };
        let mut enabled = openxr::ExtensionSet::default();
        if use_enable2 {
            enabled.khr_vulkan_enable2 = true;
        } else {
            enabled.khr_vulkan_enable = true;
        }
        let xr_instance = entry
            .create_instance(
                &openxr::ApplicationInfo {
                    application_name: "FableFlight",
                    application_version: 1,
                    engine_name: "FableFlight",
                    engine_version: 1,
                    api_version: openxr::Version::new(1, 0, 0),
                },
                &enabled,
                &[],
                &(),
            )
            .map_err(|e| anyhow::anyhow!("OpenXR instance creation failed (check that WiVRn / SteamVR / Monado is running and active_runtime.json is set): {e}"))?;
        let system = xr_instance
            .system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|e| anyhow::anyhow!("no OpenXR headset is available (connect your Meta Quest 2 via WiVRn / ALVR / SteamVR first): {e}"))?;
        let req = xr_instance
            .graphics_requirements::<openxr::Vulkan>(system)
            .map_err(|e| anyhow::anyhow!("OpenXR Vulkan requirements failed: {e}"))?;

        let vk_entry = unsafe { ash::Entry::load() }
            .map_err(|e| anyhow::anyhow!("could not load Vulkan loader (libvulkan.so.1): {e}"))?;
        let loader_ver = unsafe { vk_entry.try_enumerate_instance_version() }
            .map_err(|e| anyhow::anyhow!("vkEnumerateInstanceVersion failed: {e}"))?
            .unwrap_or(vk::API_VERSION_1_0);
        let min_vk = vk::make_api_version(
            0,
            req.min_api_version_supported.major() as u32,
            req.min_api_version_supported.minor() as u32,
            0,
        );
        let max_vk = vk::make_api_version(
            0,
            req.max_api_version_supported.major() as u32,
            req.max_api_version_supported.minor() as u32,
            0,
        );
        if loader_ver < min_vk {
            anyhow::bail!(
                "Vulkan {}.{}.{} is older than the OpenXR runtime minimum ({}.{})",
                vk::api_version_major(loader_ver),
                vk::api_version_minor(loader_ver),
                vk::api_version_patch(loader_ver),
                req.min_api_version_supported.major(),
                req.min_api_version_supported.minor(),
            );
        }
        // Stay within both wgpu-hal's max (1.3) and the OpenXR runtime's tested range (>= 1.1).
        let app_vk_ver = if loader_ver < vk::API_VERSION_1_1 {
            vk::API_VERSION_1_0
        } else {
            loader_ver.min(vk::API_VERSION_1_3).min(max_vk.max(vk::API_VERSION_1_1))
        };

        let wgpu_flags = wgpu::InstanceFlags::empty();
        let wgpu_exts = wgpu::hal::vulkan::Instance::desired_extensions(&vk_entry, app_vk_ver, wgpu_flags)
            .map_err(|e| anyhow::anyhow!("Vulkan instance extensions query failed: {e}"))?;

        let mut legacy_ext_cstrings: Vec<CString> = Vec::new();
        let mut ext_ptrs: Vec<*const std::ffi::c_char> = wgpu_exts.iter().map(|s| s.as_ptr()).collect();
        if !use_enable2 {
            let needed = xr_instance
                .vulkan_legacy_instance_extensions(system)
                .map_err(|e| anyhow::anyhow!("could not query OpenXR Vulkan instance extensions: {e}"))?;
            for name in needed.split_whitespace() {
                let cs = CString::new(name)?;
                if !wgpu_exts.iter().any(|e| *e == cs.as_c_str())
                    && !legacy_ext_cstrings.iter().any(|e| e.as_c_str() == cs.as_c_str())
                {
                    legacy_ext_cstrings.push(cs);
                }
            }
            for cs in &legacy_ext_cstrings {
                ext_ptrs.push(cs.as_ptr());
            }
        }

        let app_info = vk::ApplicationInfo::default()
            .application_name(c"FableFlight")
            .application_version(1)
            .engine_name(c"wgpu-hal")
            .engine_version(2)
            .api_version(app_vk_ver);
        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&ext_ptrs);

        let vk_instance = if use_enable2 {
            let get_proc: openxr::sys::platform::VkGetInstanceProcAddr = unsafe {
                std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr)
            };
            let raw = unsafe {
                xr_instance.create_vulkan_instance(
                    system,
                    get_proc,
                    &create_info as *const vk::InstanceCreateInfo as *const c_void,
                )
            }
            .map_err(|e| anyhow::anyhow!("xrCreateVulkanInstanceKHR failed: {e}"))?
            .map_err(|vk_err| anyhow::anyhow!("OpenXR Vulkan instance creation failed (VkResult {vk_err})"))?;
            let handle = vk::Instance::from_raw(raw as u64);
            unsafe { ash::Instance::load(vk_entry.static_fn(), handle) }
        } else {
            unsafe { vk_entry.create_instance(&create_info, None) }
                .map_err(|e| anyhow::anyhow!("vkCreateInstance for OpenXR failed: {e}"))?
        };

        let raw_phd = unsafe {
            xr_instance.vulkan_graphics_device(system, vk_instance.handle().as_raw() as *const c_void)
        }
        .map_err(|e| anyhow::anyhow!("xrGetVulkanGraphicsDevice failed: {e}"))?;
        let vk_physical_device = vk::PhysicalDevice::from_raw(raw_phd as u64);
        if vk_physical_device == vk::PhysicalDevice::null() {
            anyhow::bail!("OpenXR returned a null Vulkan physical device");
        }

        let has_nv_optimus = unsafe { vk_entry.enumerate_instance_layer_properties() }
            .map(|layers| layers.iter().any(|l| l.layer_name_as_c_str() == Ok(c"VK_LAYER_NV_optimus")))
            .unwrap_or(false);
        let hal_instance = unsafe {
            wgpu::hal::vulkan::Instance::from_raw(
                vk_entry.clone(),
                vk_instance.clone(),
                app_vk_ver,
                0,
                None,
                wgpu_exts,
                wgpu_flags,
                wgpu::MemoryBudgetThresholds::default(),
                has_nv_optimus,
                None,
            )
        }
        .map_err(|e| anyhow::anyhow!("wgpu-hal Vulkan instance wrap failed: {e}"))?;

        let exposed = hal_instance
            .expose_adapter(vk_physical_device)
            .ok_or_else(|| anyhow::anyhow!("wgpu could not expose the Vulkan GPU required by OpenXR"))?;
        let wgpu_instance = unsafe { wgpu::Instance::from_hal::<wgpu::hal::api::Vulkan>(hal_instance) };
        let adapter = unsafe { wgpu_instance.create_adapter_from_hal(exposed) };

        let runtime_name = xr_instance
            .properties()
            .map(|p| p.runtime_name)
            .unwrap_or_else(|_| "OpenXR".into());
        log::info!(
            "OpenXR Vulkan bootstrap ready ({runtime_name}, {}, Vulkan {}.{})",
            if use_enable2 { "XR_KHR_vulkan_enable2" } else { "XR_KHR_vulkan_enable" },
            vk::api_version_major(app_vk_ver),
            vk::api_version_minor(app_vk_ver),
        );

        let _ = CStr::from_bytes_with_nul; // keep import clean if unused
        Ok((
            wgpu_instance,
            adapter,
            Self {
                entry,
                instance: xr_instance,
                system,
                use_enable2,
                vk_entry,
                vk_instance,
                vk_physical_device,
            },
        ))
    }

    /// Creates the logical `VkDevice` through OpenXR (injecting all runtime-
    /// required device extensions) and wraps it into a `wgpu::Device`/`Queue`.
    pub fn create_device(
        &self,
        adapter: &wgpu::Adapter,
        desc: &wgpu::DeviceDescriptor,
    ) -> anyhow::Result<(wgpu::Device, wgpu::Queue, u32)> {
        use ash::vk::{self, Handle as _};
        use std::ffi::{CString, c_void};

        let (open_device, family_index) = unsafe {
            let hal_adapter = adapter
                .as_hal::<wgpu::hal::api::Vulkan>()
                .ok_or_else(|| anyhow::anyhow!("adapter is not a Vulkan adapter"))?;

            let qprops = self.vk_instance.get_physical_device_queue_family_properties(self.vk_physical_device);
            let family_index = qprops
                .iter()
                .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE))
                .or_else(|| qprops.iter().position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS)))
                .ok_or_else(|| anyhow::anyhow!("Vulkan GPU has no graphics queue family"))? as u32;

            let enabled_extensions = hal_adapter.required_device_extensions(desc.required_features);
            let mut enabled_phd_features = hal_adapter.physical_device_features(&enabled_extensions, desc.required_features);

            let mut legacy_ext_cstrings: Vec<CString> = Vec::new();
            let mut ext_ptrs: Vec<*const std::ffi::c_char> = enabled_extensions.iter().map(|s| s.as_ptr()).collect();
            if !self.use_enable2 {
                let needed = self
                    .instance
                    .vulkan_legacy_device_extensions(self.system)
                    .map_err(|e| anyhow::anyhow!("could not query OpenXR Vulkan device extensions: {e}"))?;
                for name in needed.split_whitespace() {
                    let cs = CString::new(name)?;
                    if !enabled_extensions.iter().any(|e| *e == cs.as_c_str())
                        && !legacy_ext_cstrings.iter().any(|e| e.as_c_str() == cs.as_c_str())
                    {
                        legacy_ext_cstrings.push(cs);
                    }
                }
                for cs in &legacy_ext_cstrings {
                    ext_ptrs.push(cs.as_ptr());
                }
            }

            let priorities = [1.0f32];
            let queue_infos = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family_index)
                .queue_priorities(&priorities)];
            let pre_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&queue_infos)
                .enabled_extension_names(&ext_ptrs);
            let dev_create_info = enabled_phd_features.add_to_device_create(pre_info);

            let ash_device = if self.use_enable2 {
                let get_proc: openxr::sys::platform::VkGetInstanceProcAddr =
                    std::mem::transmute(self.vk_entry.static_fn().get_instance_proc_addr);
                let raw = self
                    .instance
                    .create_vulkan_device(
                        self.system,
                        get_proc,
                        self.vk_physical_device.as_raw() as *const c_void,
                        &dev_create_info as *const vk::DeviceCreateInfo as *const c_void,
                    )
                    .map_err(|e| anyhow::anyhow!("xrCreateVulkanDeviceKHR failed: {e}"))?
                    .map_err(|vk_err| anyhow::anyhow!("OpenXR Vulkan device creation failed (VkResult {vk_err})"))?;
                let handle = vk::Device::from_raw(raw as u64);
                ash::Device::load(self.vk_instance.fp_v1_0(), handle)
            } else {
                self.vk_instance
                    .create_device(self.vk_physical_device, &dev_create_info, None)
                    .map_err(|e| anyhow::anyhow!("vkCreateDevice for OpenXR failed: {e}"))?
            };

            let open = hal_adapter
                .device_from_raw(
                    ash_device,
                    None,
                    &enabled_extensions,
                    desc.required_features,
                    &desc.required_limits,
                    &desc.memory_hints,
                    family_index,
                    0,
                )
                .map_err(|e| anyhow::anyhow!("wgpu-hal Vulkan device wrap failed: {e}"))?;
            (open, family_index)
        };

        let (device, queue) = unsafe { adapter.create_device_from_hal(open_device, desc) }
            .map_err(|e| anyhow::anyhow!("wgpu create_device_from_hal failed: {e}"))?;
        Ok((device, queue, family_index))
    }

    /// Creates the OpenXR Vulkan session and per-eye swapchains on `device`.
    pub fn finish(self, device: &wgpu::Device, family_index: u32) -> anyhow::Result<OpenXrRuntime> {
        use ash::vk::{self, Handle as _};
        use std::ffi::c_void;

        let hal_dev = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }
            .ok_or_else(|| anyhow::anyhow!("device is not a Vulkan device"))?;
        let raw_dev_handle = hal_dev.raw_device().handle().as_raw() as *const c_void;

        let (session, frame_wait, frame_stream) = unsafe {
            self.instance.create_session::<openxr::Vulkan>(
                self.system,
                &openxr::vulkan::SessionCreateInfo {
                    instance: self.vk_instance.handle().as_raw() as *const c_void,
                    physical_device: self.vk_physical_device.as_raw() as *const c_void,
                    device: raw_dev_handle,
                    queue_family_index: family_index,
                    queue_index: 0,
                },
            )
        }
        .map_err(|e| anyhow::anyhow!("OpenXR could not create a Vulkan session: {e}"))?;

        let configs = self
            .instance
            .enumerate_view_configuration_views(self.system, openxr::ViewConfigurationType::PRIMARY_STEREO)
            .map_err(|e| anyhow::anyhow!("could not read headset view sizes: {e}"))?;
        if configs.len() != 2 {
            anyhow::bail!("OpenXR runtime reported {} views; FableFlight needs 2", configs.len());
        }
        let eye_extents = [
            [configs[0].recommended_image_rect_width.max(1), configs[0].recommended_image_rect_height.max(1)],
            [configs[1].recommended_image_rect_width.max(1), configs[1].recommended_image_rect_height.max(1)],
        ];
        let extent = [
            eye_extents[0][0].max(eye_extents[1][0]),
            eye_extents[0][1].max(eye_extents[1][1]),
        ];
        let blend = self
            .instance
            .enumerate_environment_blend_modes(self.system, openxr::ViewConfigurationType::PRIMARY_STEREO)
            .map_err(|e| anyhow::anyhow!("could not query headset blend modes: {e}"))?
            .into_iter()
            .next()
            .unwrap_or(openxr::EnvironmentBlendMode::OPAQUE);
        let space = session
            .create_reference_space(openxr::ReferenceSpaceType::LOCAL, openxr::Posef::IDENTITY)
            .map_err(|e| anyhow::anyhow!("could not create headset reference space: {e}"))?;

        let runtime_formats = session
            .enumerate_swapchain_formats()
            .map_err(|e| anyhow::anyhow!("could not query headset swapchain formats: {e}"))?;
        // Prefer sRGB formats: OpenXR compositors treat sRGB swapchains as
        // display-encoded sRGB and UNORM swapchains as linear (which washes out
        // gamma-encoded colours if written directly).
        let candidates: [(vk::Format, wgpu::TextureFormat); 4] = [
            (vk::Format::R8G8B8A8_SRGB, wgpu::TextureFormat::Rgba8UnormSrgb),
            (vk::Format::B8G8R8A8_SRGB, wgpu::TextureFormat::Bgra8UnormSrgb),
            (vk::Format::R8G8B8A8_UNORM, wgpu::TextureFormat::Rgba8Unorm),
            (vk::Format::B8G8R8A8_UNORM, wgpu::TextureFormat::Bgra8Unorm),
        ];
        let (vk_fmt, texture_format) = candidates
            .into_iter()
            .find(|(vkf, _)| runtime_formats.contains(&(vkf.as_raw() as u32)))
            .ok_or_else(|| anyhow::anyhow!("the OpenXR runtime exposed no supported 8-bit RGBA/BGRA swapchain format (available: {runtime_formats:?})"))?;

        let make_eye_swapchain = |eye: usize| -> anyhow::Result<(openxr::Swapchain<openxr::Vulkan>, Vec<wgpu::Texture>, Vec<wgpu::TextureView>)> {
            let [w, h] = eye_extents[eye];
            let swapchain = session
                .create_swapchain(&openxr::SwapchainCreateInfo {
                    create_flags: openxr::SwapchainCreateFlags::EMPTY,
                    usage_flags: openxr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                    format: vk_fmt.as_raw() as u32,
                    sample_count: 1,
                    width: w,
                    height: h,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                })
                .map_err(|e| anyhow::anyhow!("could not create eye {eye} OpenXR swapchain: {e}"))?;
            let images = swapchain
                .enumerate_images()
                .map_err(|e| anyhow::anyhow!("could not enumerate eye {eye} swapchain images: {e}"))?;
            let mut textures = Vec::with_capacity(images.len());
            let mut views = Vec::with_capacity(images.len());
            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let label = if eye == 0 { "OpenXR left eye" } else { "OpenXR right eye" };
            for raw in images {
                let hal_desc = wgpu::hal::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: texture_format,
                    usage: wgpu::TextureUses::COLOR_TARGET,
                    memory_flags: wgpu::hal::MemoryFlags::empty(),
                    view_formats: Vec::new(),
                };
                // OpenXR owns the VkImage and its backing memory; pass a no-op
                // drop callback + TextureMemory::External so wgpu never calls
                // vkDestroyImage on it. OpenXR hands acquired images in
                // VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL (COLOR_TARGET).
                let hal_tex = unsafe {
                    hal_dev.texture_from_raw(
                        vk::Image::from_raw(raw),
                        &hal_desc,
                        Some(Box::new(|| {})),
                        wgpu::hal::vulkan::TextureMemory::External,
                    )
                };
                let desc = wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: texture_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                };
                let texture = unsafe {
                    device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
                        hal_tex,
                        &desc,
                        wgpu::TextureUses::COLOR_TARGET,
                    )
                };
                let tv = texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some(label),
                    ..Default::default()
                });
                textures.push(texture);
                views.push(tv);
            }
            Ok((swapchain, textures, views))
        };

        let (sc0, tex0, view0) = make_eye_swapchain(0)?;
        let (sc1, tex1, view1) = make_eye_swapchain(1)?;
        let runtime_name = self
            .instance
            .properties()
            .map(|p| p.runtime_name)
            .unwrap_or_else(|_| "OpenXR runtime".into());
        log::info!(
            "OpenXR Vulkan ready: {runtime_name}, {}×{} per eye ({texture_format:?})",
            extent[0],
            extent[1],
        );

        Ok(OpenXrRuntime {
            views: [view0, view1],
            _textures: [tex0, tex1],
            swapchains: [sc0, sc1],
            space,
            frame_stream,
            frame_wait,
            session,
            _system: self.system,
            instance: self.instance,
            _entry: self.entry,
            extent,
            eye_extents,
            format: texture_format,
            blend,
            runtime_name,
            session_state: openxr::SessionState::UNKNOWN,
            running: false,
            exiting: false,
            active_frame: None,
            tracking_origin: None,
            last_head_pose: None,
        })
    }
}

#[cfg(windows)]
impl OpenXrRuntime {
    pub fn new(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> anyhow::Result<Self> {
        use windows::core::Interface;

        let entry = unsafe { openxr::Entry::load(&()) }
            .map_err(|e| anyhow::anyhow!("OpenXR loader unavailable: {e}"))?;
        let available = entry
            .enumerate_extensions()
            .map_err(|e| anyhow::anyhow!("could not query OpenXR extensions: {e}"))?;
        if !available.khr_d3d12_enable {
            anyhow::bail!("the active OpenXR runtime does not support D3D12");
        }
        let mut enabled = openxr::ExtensionSet::default();
        enabled.khr_d3d12_enable = true;
        let instance = entry
            .create_instance(
                &openxr::ApplicationInfo {
                    application_name: "FableFlight",
                    application_version: 1,
                    engine_name: "FableFlight",
                    engine_version: 1,
                    api_version: openxr::Version::new(1, 0, 0),
                },
                &enabled,
                &[],
                &(),
            )
            .map_err(|e| anyhow::anyhow!("OpenXR could not start: {e}"))?;
        let system = instance
            .system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|e| anyhow::anyhow!("no OpenXR headset is available: {e}"))?;
        let requirements = instance
            .graphics_requirements::<openxr::D3D12>(system)
            .map_err(|e| anyhow::anyhow!("OpenXR D3D12 requirements failed: {e}"))?;

        let hal_adapter = unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| anyhow::anyhow!("the game did not select a Direct3D 12 adapter"))?;
        let adapter_desc = unsafe { hal_adapter.raw_adapter().GetDesc2() }
            .map_err(|e| anyhow::anyhow!("could not read the graphics adapter id: {e}"))?;
        if adapter_desc.AdapterLuid.LowPart != requirements.adapter_luid.LowPart
            || adapter_desc.AdapterLuid.HighPart != requirements.adapter_luid.HighPart
        {
            anyhow::bail!("OpenXR requires a different graphics adapter; set Windows graphics preferences so FableFlight and the headset runtime use the same GPU");
        }

        let hal = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| anyhow::anyhow!("the game did not start on Direct3D 12"))?;
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Dx12 {
            anyhow::bail!("OpenXR D3D12 needs the game to use the Direct3D 12 backend");
        }
        let raw_device = hal.raw_device().as_raw() as *mut _;
        let raw_queue = hal.raw_queue().as_raw() as *mut _;
        let (session, frame_wait, frame_stream) = unsafe {
            instance.create_session::<openxr::D3D12>(
                system,
                &openxr::d3d::SessionCreateInfoD3D12 { device: raw_device, queue: raw_queue },
            )
        }
        .map_err(|e| anyhow::anyhow!("OpenXR could not create a D3D12 session: {e}"))?;

        let configs = instance
            .enumerate_view_configuration_views(system, openxr::ViewConfigurationType::PRIMARY_STEREO)
            .map_err(|e| anyhow::anyhow!("could not read headset view sizes: {e}"))?;
        if configs.len() != 2 {
            anyhow::bail!("this OpenXR runtime reported {} views; FableFlight needs two", configs.len());
        }
        let eye_extents = [
            [configs[0].recommended_image_rect_width.max(1), configs[0].recommended_image_rect_height.max(1)],
            [configs[1].recommended_image_rect_width.max(1), configs[1].recommended_image_rect_height.max(1)],
        ];
        let extent = [
            eye_extents[0][0].max(eye_extents[1][0]),
            eye_extents[0][1].max(eye_extents[1][1]),
        ];
        let blend = instance
            .enumerate_environment_blend_modes(system, openxr::ViewConfigurationType::PRIMARY_STEREO)
            .map_err(|e| anyhow::anyhow!("could not query headset blend modes: {e}"))?
            .into_iter()
            .next()
            .unwrap_or(openxr::EnvironmentBlendMode::OPAQUE);
        let space = session
            .create_reference_space(openxr::ReferenceSpaceType::LOCAL, openxr::Posef::IDENTITY)
            .map_err(|e| anyhow::anyhow!("could not create headset reference space: {e}"))?;

        let runtime_formats = session
            .enumerate_swapchain_formats()
            .map_err(|e| anyhow::anyhow!("could not query headset image formats: {e}"))?;
        let _ = surface_format;
        // Prefer sRGB formats (29 = R8G8B8A8_UNORM_SRGB, 91 = B8G8R8A8_UNORM_SRGB), then UNORM.
        let candidates: [(u32, wgpu::TextureFormat); 4] = [
            (29, wgpu::TextureFormat::Rgba8UnormSrgb),
            (91, wgpu::TextureFormat::Bgra8UnormSrgb),
            (28, wgpu::TextureFormat::Rgba8Unorm),
            (87, wgpu::TextureFormat::Bgra8Unorm),
        ];
        let (dxgi_fmt, texture_format) = candidates
            .into_iter()
            .find(|(f, _)| runtime_formats.contains(f))
            .ok_or_else(|| anyhow::anyhow!("the OpenXR runtime has no supported 8-bit RGBA/BGRA swapchain format"))?;

        let make_eye_swapchain = |eye: usize| -> anyhow::Result<(openxr::Swapchain<openxr::D3D12>, Vec<wgpu::Texture>, Vec<wgpu::TextureView>)> {
            let [w, h] = eye_extents[eye];
            let swapchain = session
                .create_swapchain(&openxr::SwapchainCreateInfo {
                    create_flags: openxr::SwapchainCreateFlags::EMPTY,
                    usage_flags: openxr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                    format: dxgi_fmt,
                    sample_count: 1,
                    width: w,
                    height: h,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                })
                .map_err(|e| anyhow::anyhow!("could not create eye {eye} headset swapchain: {e}"))?;
            let images = swapchain
                .enumerate_images()
                .map_err(|e| anyhow::anyhow!("could not get eye {eye} headset images: {e}"))?;
            let mut textures = Vec::with_capacity(images.len());
            let mut views = Vec::with_capacity(images.len());
            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let label = if eye == 0 { "OpenXR left eye" } else { "OpenXR right eye" };
            for raw in images {
                let borrowed = unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&raw) }
                    .ok_or_else(|| anyhow::anyhow!("OpenXR returned a null swapchain image"))?;
                let resource = borrowed.clone();
                let hal_texture = unsafe {
                    wgpu::hal::dx12::Device::texture_from_raw(
                        resource,
                        texture_format,
                        wgpu::TextureDimension::D2,
                        size,
                        1,
                        1,
                    )
                };
                let desc = wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: texture_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                };
                let texture = unsafe {
                    device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
                        hal_texture,
                        &desc,
                        wgpu::TextureUses::COLOR_TARGET,
                    )
                };
                let tv = texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some(label),
                    ..Default::default()
                });
                textures.push(texture);
                views.push(tv);
            }
            Ok((swapchain, textures, views))
        };

        let (sc0, tex0, view0) = make_eye_swapchain(0)?;
        let (sc1, tex1, view1) = make_eye_swapchain(1)?;
        let runtime_name = instance
            .properties()
            .map(|p| p.runtime_name)
            .unwrap_or_else(|_| "OpenXR runtime".into());
        log::info!(
            "OpenXR D3D12 ready: {runtime_name}, {}×{} per eye ({texture_format:?})",
            extent[0],
            extent[1],
        );

        Ok(Self {
            views: [view0, view1],
            _textures: [tex0, tex1],
            swapchains: [sc0, sc1],
            space,
            frame_stream,
            frame_wait,
            session,
            _system: system,
            instance,
            _entry: entry,
            extent,
            eye_extents,
            format: texture_format,
            blend,
            runtime_name,
            session_state: openxr::SessionState::UNKNOWN,
            running: false,
            exiting: false,
            active_frame: None,
            tracking_origin: None,
            last_head_pose: None,
        })
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl OpenXrRuntime {
    pub fn extent(&self) -> [u32; 2] { self.extent }
    pub fn format(&self) -> wgpu::TextureFormat { self.format }
    pub fn head_pose(&self) -> Option<(Vec3, Quat)> { self.last_head_pose }
    pub fn is_running(&self) -> bool { self.running }
    pub fn is_exiting(&self) -> bool { self.exiting }

    pub fn recenter(&mut self) {
        self.tracking_origin = None;
    }

    pub fn status_text(&self) -> String {
        let state = match self.session_state {
            openxr::SessionState::FOCUSED => "active",
            openxr::SessionState::VISIBLE => "visible",
            openxr::SessionState::SYNCHRONIZED => "synchronized",
            openxr::SessionState::READY => "ready",
            openxr::SessionState::IDLE => "waiting for headset",
            openxr::SessionState::STOPPING => "stopping",
            openxr::SessionState::LOSS_PENDING | openxr::SessionState::EXITING => "disconnected",
            _ => "initializing",
        };
        format!(
            "{} · {}×{}/eye ({})",
            self.runtime_name, self.extent[0], self.extent[1], state
        )
    }

    pub fn poll_begin(&mut self) -> anyhow::Result<Option<XrFrame>> {
        let mut event = openxr::EventDataBuffer::new();
        while let Some(ev) = self.instance.poll_event(&mut event)? {
            match ev {
                openxr::Event::SessionStateChanged(s) => {
                    self.session_state = s.state();
                    match s.state() {
                        openxr::SessionState::READY => {
                            self.session.begin(openxr::ViewConfigurationType::PRIMARY_STEREO)?;
                            self.running = true;
                        }
                        openxr::SessionState::STOPPING => {
                            if self.running {
                                self.session.end()?;
                            }
                            self.running = false;
                        }
                        openxr::SessionState::EXITING | openxr::SessionState::LOSS_PENDING => {
                            self.running = false;
                            self.exiting = true;
                        }
                        _ => {}
                    }
                }
                openxr::Event::ReferenceSpaceChangePending(_) => {
                    self.tracking_origin = None;
                }
                _ => {}
            }
        }
        if !self.running {
            return Ok(None);
        }

        let state = self.frame_wait.wait()?;
        self.frame_stream.begin()?;
        // Register active_frame right after frame_stream.begin() succeeds so
        // finish() always balances it with frame_stream.end() even on error.
        self.active_frame = Some(ActiveXrFrame {
            display_time: state.predicted_display_time,
            views: Vec::new(),
            images: [None, None],
            should_render: false,
        });

        if !state.should_render {
            return Ok(Some(XrFrame {
                display_time: state.predicted_display_time,
                views: Vec::new(),
                should_render: false,
            }));
        }

        let (flags, raw_views) = self.session.locate_views(
            openxr::ViewConfigurationType::PRIMARY_STEREO,
            state.predicted_display_time,
            &self.space,
        )?;
        let pose_valid = flags.contains(openxr::ViewStateFlags::ORIENTATION_VALID)
            && flags.contains(openxr::ViewStateFlags::POSITION_VALID)
            && raw_views.len() == 2;
        if !pose_valid {
            return Ok(Some(XrFrame {
                display_time: state.predicted_display_time,
                views: Vec::new(),
                should_render: false,
            }));
        }

        for eye in 0..2 {
            let idx = self.swapchains[eye].acquire_image()? as usize;
            if let Some(af) = self.active_frame.as_mut() {
                af.images[eye] = Some(idx);
            }
            self.swapchains[eye].wait_image(openxr::Duration::INFINITE)?;
        }

        let mut tracked_views = raw_views.clone();
        if let [left, right] = tracked_views.as_mut_slice() {
            let lp = Vec3::new(left.pose.position.x, left.pose.position.y, left.pose.position.z);
            let rp = Vec3::new(right.pose.position.x, right.pose.position.y, right.pose.position.z);
            let mid = (lp + rp) * 0.5;
            let q_left = safe_quat(
                Quat::from_xyzw(
                    left.pose.orientation.x,
                    left.pose.orientation.y,
                    left.pose.orientation.z,
                    left.pose.orientation.w,
                ),
                Quat::IDENTITY,
            );
            let (origin_q, origin_p) = *self.tracking_origin.get_or_insert_with(|| (yaw_only(q_left), mid));
            let inv_origin = origin_q.conjugate();
            for view in tracked_views.iter_mut() {
                let q = safe_quat(
                    Quat::from_xyzw(
                        view.pose.orientation.x,
                        view.pose.orientation.y,
                        view.pose.orientation.z,
                        view.pose.orientation.w,
                    ),
                    Quat::IDENTITY,
                );
                let p = Vec3::new(view.pose.position.x, view.pose.position.y, view.pose.position.z);
                let rel_q = (inv_origin * q).normalize();
                let rel_p = inv_origin * (p - origin_p);
                view.pose.orientation = openxr::Quaternionf { x: rel_q.x, y: rel_q.y, z: rel_q.z, w: rel_q.w };
                view.pose.position = openxr::Vector3f { x: rel_p.x, y: rel_p.y, z: rel_p.z };
            }
            let p0 = Vec3::new(tracked_views[0].pose.position.x, tracked_views[0].pose.position.y, tracked_views[0].pose.position.z);
            let p1 = Vec3::new(tracked_views[1].pose.position.x, tracked_views[1].pose.position.y, tracked_views[1].pose.position.z);
            let q0 = Quat::from_xyzw(tracked_views[0].pose.orientation.x, tracked_views[0].pose.orientation.y, tracked_views[0].pose.orientation.z, tracked_views[0].pose.orientation.w);
            let q1 = Quat::from_xyzw(tracked_views[1].pose.orientation.x, tracked_views[1].pose.orientation.y, tracked_views[1].pose.orientation.z, tracked_views[1].pose.orientation.w);
            let head_q = if q0.dot(q1) < 0.0 { q0.slerp(-q1, 0.5) } else { q0.slerp(q1, 0.5) }.normalize();
            self.last_head_pose = Some(((p0 + p1) * 0.5, head_q));
        }

        if let Some(af) = self.active_frame.as_mut() {
            af.views = raw_views;
            af.should_render = true;
        }
        Ok(Some(XrFrame {
            display_time: state.predicted_display_time,
            views: tracked_views,
            should_render: true,
        }))
    }

    pub fn target(&self, eye: usize) -> Option<&wgpu::TextureView> {
        let idx = self.active_frame.as_ref()?.images.get(eye).copied().flatten()?;
        self.views.get(eye)?.get(idx)
    }

    pub fn finish(&mut self) -> anyhow::Result<()> {
        let Some(frame) = self.active_frame.take() else { return Ok(()); };
        let mut first_err: Option<anyhow::Error> = None;
        for eye in 0..2 {
            if frame.images[eye].is_some() {
                if let Err(e) = self.swapchains[eye].release_image() {
                    if first_err.is_none() {
                        first_err = Some(e.into());
                    }
                }
            }
        }
        if frame.should_render && frame.images[0].is_some() && frame.images[1].is_some() && frame.views.len() == 2 {
            let projection_views = [0, 1].map(|eye| {
                let rect = openxr::Rect2Di {
                    offset: openxr::Offset2Di { x: 0, y: 0 },
                    extent: openxr::Extent2Di {
                        width: self.eye_extents[eye][0] as i32,
                        height: self.eye_extents[eye][1] as i32,
                    },
                };
                openxr::CompositionLayerProjectionView::new()
                    .pose(frame.views[eye].pose)
                    .fov(frame.views[eye].fov)
                    .sub_image(
                        openxr::SwapchainSubImage::new()
                            .swapchain(&self.swapchains[eye])
                            .image_array_index(0)
                            .image_rect(rect),
                    )
            });
            let projection = openxr::CompositionLayerProjection::new()
                .space(&self.space)
                .views(&projection_views);
            self.frame_stream.end(frame.display_time, self.blend, &[&projection])?;
        } else {
            self.frame_stream.end(frame.display_time, self.blend, &[])?;
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        Ok(())
    }
}

/// Standard Quest 2 physical hardware specifications.
pub const QUEST_2_IPD_PRESET_1: f32 = 0.058; // 58 mm (Setting 1 - Narrow)
pub const QUEST_2_IPD_PRESET_2: f32 = 0.063; // 63 mm (Setting 2 - Standard / Default)
pub const QUEST_2_IPD_PRESET_3: f32 = 0.068; // 68 mm (Setting 3 - Wide)
pub const QUEST_2_DEFAULT_FOV_V: f32 = 98.0; // degrees vertical FOV
pub const QUEST_2_DEFAULT_FOV_H: f32 = 104.0; // degrees horizontal FOV

/// Eye identifier for stereoscopic rendering.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Eye {
    Left,
    Right,
}

/// Head tracking pose in local headset space.
#[derive(Copy, Clone, Debug)]
pub struct HeadPose {
    pub position: DVec3,
    pub orientation: DQuat,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
}

impl Default for HeadPose {
    fn default() -> Self {
        Self {
            position: DVec3::ZERO,
            orientation: DQuat::IDENTITY,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        }
    }
}

/// Clamps the persistent VR cockpit seat offset (body frame, metres: +X right,
/// +Y up, -Z forward) to a comfortable range inside the cabin.
pub fn clamp_seat_offset(v: Vec3) -> Vec3 {
    Vec3::new(
        if v.x.is_finite() { v.x.clamp(-0.25, 0.25) } else { 0.0 },
        if v.y.is_finite() { v.y.clamp(-0.25, 0.25) } else { 0.0 },
        if v.z.is_finite() { v.z.clamp(-0.35, 0.35) } else { 0.0 },
    )
}

/// State for VR settings and desktop preview fallback when no OpenXR headset is active.
#[derive(Clone, Debug)]
pub struct VrSystem {
    pub enabled: bool,
    /// Interpupillary distance in metres (typically 0.058 - 0.068 m).
    pub ipd: f32,
    /// Persistent cockpit seat position offset in aircraft body frame (metres: +X right, +Y up, -Z forward).
    pub seat_offset: Vec3,
    /// Vertical field of view in degrees.
    pub fov_v: f32,
    /// Horizontal field of view in degrees.
    pub fov_h: f32,
    /// Whether a likely OpenXR runtime manifest was found on this machine.
    pub runtime_detected: bool,
    /// Runtime name / description.
    pub runtime_name: String,
    /// Current head tracking pose.
    pub pose: HeadPose,
    /// Recentering offset.
    pub center_yaw: f64,
    pub center_pos: DVec3,
}

impl Default for VrSystem {
    fn default() -> Self {
        Self::new(false)
    }
}

impl VrSystem {
    /// Creates VR state and checks for a configured OpenXR runtime manifest.
    pub fn new(enabled: bool) -> Self {
        let (detected, name) = detect_openxr_runtime();
        Self {
            enabled,
            ipd: QUEST_2_IPD_PRESET_2,
            seat_offset: Vec3::ZERO,
            fov_v: QUEST_2_DEFAULT_FOV_V,
            fov_h: QUEST_2_DEFAULT_FOV_H,
            runtime_detected: detected,
            runtime_name: name,
            pose: HeadPose::default(),
            center_yaw: 0.0,
            center_pos: DVec3::ZERO,
        }
    }

    /// Shifts the persistent VR cockpit seat position by `delta` (body frame, metres).
    pub fn nudge_seat(&mut self, delta: Vec3) {
        self.seat_offset = clamp_seat_offset(self.seat_offset + delta);
    }

    /// Resets the persistent VR cockpit seat offset back to the aircraft's default eye point.
    pub fn reset_seat(&mut self) {
        self.seat_offset = Vec3::ZERO;
    }

    /// Recenter the headset view to the current physical forward orientation and position.
    pub fn recenter(&mut self) {
        self.center_yaw = self.pose.yaw;
        self.center_pos = self.pose.position;
    }

    /// Update tracking state from inputs or OpenXR poll.
    pub fn update(&mut self, mouse_look: (f64, f64), _dt: f64) {
        if !self.enabled {
            return;
        }
        self.pose.yaw = mouse_look.0 - self.center_yaw;
        self.pose.pitch = mouse_look.1;
        let q_yaw = DQuat::from_rotation_y(self.pose.yaw);
        let q_pitch = DQuat::from_rotation_x(self.pose.pitch);
        let q_roll = DQuat::from_rotation_z(self.pose.roll);
        self.pose.orientation = q_yaw * q_pitch * q_roll;
    }

    /// Calculate the eye position for stereoscopic rendering.
    pub fn eye_position(&self, head_pos: DVec3, head_right: DVec3, eye: Eye) -> DVec3 {
        let half_ipd = (self.ipd * 0.5) as f64;
        match eye {
            Eye::Left => head_pos - head_right * half_ipd,
            Eye::Right => head_pos + head_right * half_ipd,
        }
    }

    /// Computes the projection matrix for the given eye.
    pub fn eye_projection(&self, eye: Eye, aspect: f32, near: f32) -> Mat4 {
        let fov_rad = self.fov_v.to_radians();
        let f = 1.0 / (fov_rad * 0.5).tan();
        let _ = eye;
        Mat4::from_cols_array(&[
            f / aspect, 0.0, 0.0, 0.0,
            0.0, f, 0.0, 0.0,
            0.0, 0.0, 0.0, -1.0,
            0.0, 0.0, near, 0.0,
        ])
    }
}

/// Reads the runtime name or library path from an OpenXR runtime manifest JSON.
fn describe_runtime_manifest(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    if let Some(name) = json.get("runtime").and_then(|r| r.get("name")).and_then(|n| n.as_str()) {
        if !name.trim().is_empty() {
            return Some(name.trim().to_string());
        }
    }
    if let Some(lib) = json.get("runtime").and_then(|r| r.get("library_path")).and_then(|l| l.as_str()) {
        let low = lib.to_ascii_lowercase();
        if low.contains("wivrn") {
            return Some("WiVRn OpenXR".into());
        }
        if low.contains("monado") {
            return Some("Monado OpenXR".into());
        }
        if low.contains("steam") || low.contains("vrclient") {
            return Some("SteamVR OpenXR".into());
        }
        if low.contains("alvr") {
            return Some("ALVR OpenXR".into());
        }
        if !lib.trim().is_empty() {
            return Some(format!("OpenXR ({lib})"));
        }
    }
    Some(format!("OpenXR ({})", path.display()))
}

/// Looks for a configured OpenXR runtime manifest on Linux or Windows.
fn detect_openxr_runtime() -> (bool, String) {
    if let Ok(xr_path) = std::env::var("XR_RUNTIME_JSON") {
        let p = Path::new(&xr_path);
        if p.exists() {
            let desc = describe_runtime_manifest(p).unwrap_or_else(|| format!("Custom OpenXR Runtime: {xr_path}"));
            return (true, desc);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut candidates = Vec::new();
        if let Some(cfg) = dirs::config_dir() {
            candidates.push(cfg.join("openxr/1/active_runtime.json"));
        }
        candidates.push(Path::new("/etc/xdg/openxr/1/active_runtime.json").to_path_buf());
        candidates.push(Path::new("/usr/share/openxr/1/active_runtime.json").to_path_buf());
        for p in candidates {
            if p.exists() {
                let desc = describe_runtime_manifest(&p).unwrap_or_else(|| format!("OpenXR ({})", p.display()));
                return (true, desc);
            }
        }
    }

    #[cfg(windows)]
    {
        let oculus_link_path = Path::new(r"C:\Program Files\Oculus\Support\oculus-runtime\oculus_openxr_64.json");
        if oculus_link_path.exists() {
            return (true, "Meta Quest Link (Oculus OpenXR)".to_string());
        }
        let steamvr_openxr = Path::new(r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR\steamxr_win64.json");
        if steamvr_openxr.exists() {
            return (true, "SteamVR OpenXR (Quest / PC VR)".to_string());
        }
    }

    (false, "No OpenXR runtime detected".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vr_initialization() {
        let vr = VrSystem::new(true);
        assert!(vr.enabled);
        assert_eq!(vr.ipd, QUEST_2_IPD_PRESET_2);
        assert_eq!(vr.fov_v, QUEST_2_DEFAULT_FOV_V);
        assert!(!vr.runtime_name.is_empty());
    }

    #[test]
    fn test_vr_eye_separation() {
        let vr = VrSystem::new(true);
        let head = DVec3::new(10.0, 5.0, -2.0);
        let right = DVec3::X;
        let left_eye = vr.eye_position(head, right, Eye::Left);
        let right_eye = vr.eye_position(head, right, Eye::Right);

        let dist = (right_eye - left_eye).length();
        assert!((dist - vr.ipd as f64).abs() < 1e-6);
        assert_eq!(left_eye.x, head.x - (vr.ipd as f64 * 0.5));
        assert_eq!(right_eye.x, head.x + (vr.ipd as f64 * 0.5));
    }

    #[test]
    fn test_vr_head_pose_and_recentering() {
        let mut vr = VrSystem::new(true);
        vr.update((1.5, 0.4), 0.016);
        assert!((vr.pose.yaw - 1.5).abs() < 1e-6);
        assert!((vr.pose.pitch - 0.4).abs() < 1e-6);

        vr.recenter();
        assert!((vr.center_yaw - 1.5).abs() < 1e-6);

        vr.update((1.5, 0.4), 0.016);
        assert!((vr.pose.yaw - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_vr_eye_projection() {
        let vr = VrSystem::new(true);
        let proj = vr.eye_projection(Eye::Left, 1.0, 0.1);
        let cols = proj.to_cols_array();
        assert!(cols.iter().all(|v| v.is_finite()));
        assert!(cols[0] > 0.0);
        assert!(cols[5] > 0.0);
    }

    #[test]
    fn test_yaw_only_preserves_horizon() {
        // Looking 60 deg left and 25 deg down with 10 deg head roll.
        let q = Quat::from_rotation_y(1.05) * Quat::from_rotation_x(-0.44) * Quat::from_rotation_z(0.17);
        let y = yaw_only(q);
        // Removing yaw_only aligns horizontal forward with -Z without touching gravity up.
        let recentered = y.conjugate() * q;
        let fwd = recentered * Vec3::NEG_Z;
        assert!(fwd.x.abs() < 1e-4, "recentered forward x should be 0, got {}", fwd.x);
        assert!(fwd.z < 0.0, "recentered forward z should be negative");
        assert!((y * Vec3::Y - Vec3::Y).length() < 1e-5, "yaw_only must not tilt Y");
    }

    #[test]
    fn test_asymmetric_projection_rev_corners() {
        let fov = openxr::Fovf {
            angle_left: -0.95,
            angle_right: 0.78,
            angle_up: 0.85,
            angle_down: -0.82,
        };
        let near = 0.03f32;
        let p = asymmetric_projection_rev(fov, near);
        // Point on the near plane at the right/up edge maps to NDC (+1, +1, 1).
        let pt = glam::Vec4::new(fov.angle_right.tan() * near, fov.angle_up.tan() * near, -near, 1.0);
        let clip = p * pt;
        let ndc = clip.truncate() / clip.w;
        assert!((ndc.x - 1.0).abs() < 1e-5, "ndc.x = {}", ndc.x);
        assert!((ndc.y - 1.0).abs() < 1e-5, "ndc.y = {}", ndc.y);
        assert!((ndc.z - 1.0).abs() < 1e-5, "ndc.z = {}", ndc.z);
        // Point on left/down edge at far distance maps to NDC (-1, -1, near/d).
        let d = 100.0f32;
        let pt2 = glam::Vec4::new(fov.angle_left.tan() * d, fov.angle_down.tan() * d, -d, 1.0);
        let clip2 = p * pt2;
        let ndc2 = clip2.truncate() / clip2.w;
        assert!((ndc2.x + 1.0).abs() < 1e-5, "ndc2.x = {}", ndc2.x);
        assert!((ndc2.y + 1.0).abs() < 1e-5, "ndc2.y = {}", ndc2.y);
        assert!((ndc2.z - near / d).abs() < 1e-5, "ndc2.z = {}", ndc2.z);
    }

    #[test]
    fn test_eye_matrices_stereo_at_90_deg_head_turn_and_view_ray_uncorrupted() {
        let fov = openxr::Fovf {
            angle_left: -0.85,
            angle_right: 0.85,
            angle_up: 0.85,
            angle_down: -0.85,
        };
        let near = 0.03f32;
        // Pilot turns head 90 deg right in cockpit: forward is +X, right is +Z.
        let head_rot = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        let base = XrViewBase { cockpit_rot: Quat::IDENTITY, head_pos: Vec3::ZERO };
        let half_ipd = 0.0315f32;
        let right_eye_pos = head_rot * Vec3::new(half_ipd, 0.0, 0.0); // (0, 0, +0.0315)
        let left_eye_pos = head_rot * Vec3::new(-half_ipd, 0.0, 0.0); // (0, 0, -0.0315)

        let (vp_l, inv_l) = eye_matrices(base, left_eye_pos, head_rot, fov, near);
        let (vp_r, inv_r) = eye_matrices(base, right_eye_pos, head_rot, fov, near);

        // Object 1 m straight ahead of where the pilot is looking: (+1, 0, 0).
        // Left eye should see it slightly to the RIGHT (+ndc.x), right eye to the LEFT (-ndc.x).
        let obj = glam::Vec4::new(1.0, 0.0, 0.0, 1.0);
        let nl = { let c = vp_l * obj; c.x / c.w };
        let nr = { let c = vp_r * obj; c.x / c.w };
        assert!(nl > 0.02 && nr < -0.02 && (nl + nr).abs() < 1e-5, "stereo broken: nl={nl}, nr={nr}");

        // Center screen view_ray (ndc 0,0,1,1) must point strictly along +X (NOT skewed by IPD/near!).
        let rl = { let p = inv_l * glam::Vec4::new(0.0, 0.0, 1.0, 1.0); (p.truncate() / p.w).normalize() };
        let rr = { let p = inv_r * glam::Vec4::new(0.0, 0.0, 1.0, 1.0); (p.truncate() / p.w).normalize() };
        assert!((rl - Vec3::X).length() < 1e-5, "left view_ray skewed: {rl:?}");
        assert!((rr - Vec3::X).length() < 1e-5, "right view_ray skewed: {rr:?}");
    }

    #[test]
    fn test_vr_seat_offset_nudge_clamp_and_reset() {
        let mut vr = VrSystem::new(true);
        assert_eq!(vr.seat_offset, Vec3::ZERO);

        // Moving back (+Z) shifts seat rearward; moving forward (-Z) shifts toward the panel.
        vr.nudge_seat(Vec3::new(0.0, 0.025, 0.06));
        assert!((vr.seat_offset - Vec3::new(0.0, 0.025, 0.06)).length() < 1e-6);

        // Extreme values and non-finite inputs clamp cleanly.
        vr.nudge_seat(Vec3::new(1.0, -1.0, 1.0));
        assert_eq!(vr.seat_offset, Vec3::new(0.25, -0.25, 0.35));
        assert_eq!(clamp_seat_offset(Vec3::new(f32::NAN, f32::INFINITY, -0.10)), Vec3::new(0.0, 0.0, -0.10));

        vr.reset_seat();
        assert_eq!(vr.seat_offset, Vec3::ZERO);
    }
}
