//! OpenXR runtime and first-person VR camera helpers.

use glam::{DQuat, DVec3, Mat4};
use std::path::Path;

/// Windows PC VR host. The renderer supplies its D3D12 device so OpenXR and
/// wgpu use the same GPU. See Khronos' OpenXR D3D12 binding and the OpenXR
/// frame loop: https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html
#[cfg(windows)]
pub struct OpenXrRuntime {
    _entry: openxr::Entry,
    instance: openxr::Instance,
    system: openxr::SystemId,
    session: openxr::Session<openxr::D3D12>,
    frame_wait: openxr::FrameWaiter,
    frame_stream: openxr::FrameStream<openxr::D3D12>,
    space: openxr::Space,
    swapchain: openxr::Swapchain<openxr::D3D12>,
    textures: Vec<wgpu::Texture>,
    views: Vec<[wgpu::TextureView; 2]>,
    extent: [u32; 2],
    format: wgpu::TextureFormat,
    blend: openxr::EnvironmentBlendMode,
    running: bool,
    active_frame: Option<ActiveXrFrame>,
    tracking_origin: Option<(glam::Quat, glam::Vec3)>,
    error: Option<String>,
}

#[cfg(windows)]
#[derive(Clone)]
pub struct XrFrame {
    pub display_time: openxr::Time,
    pub views: Vec<openxr::View>,
    pub image: Option<usize>,
    pub should_render: bool,
}

#[cfg(windows)]
struct ActiveXrFrame {
    display_time: openxr::Time,
    views: Vec<openxr::View>,
    image: Option<usize>,
    should_render: bool,
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
        let extent = [
            configs.iter().map(|v| v.recommended_image_rect_width).max().unwrap_or(1),
            configs.iter().map(|v| v.recommended_image_rect_height).max().unwrap_or(1),
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
        let (format, texture_format) = match surface_format {
            wgpu::TextureFormat::Bgra8Unorm if runtime_formats.contains(&87) => (87, wgpu::TextureFormat::Bgra8Unorm),
            _ if runtime_formats.contains(&28) => (28, wgpu::TextureFormat::Rgba8Unorm),
            _ if runtime_formats.contains(&87) => (87, wgpu::TextureFormat::Bgra8Unorm),
            _ => anyhow::bail!("the OpenXR runtime has no 8-bit RGBA swapchain format"),
        };
        let swapchain = session
            .create_swapchain(&openxr::SwapchainCreateInfo {
                create_flags: openxr::SwapchainCreateFlags::EMPTY,
                usage_flags: openxr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                format,
                sample_count: 1,
                width: extent[0],
                height: extent[1],
                face_count: 1,
                array_size: 2,
                mip_count: 1,
            })
            .map_err(|e| anyhow::anyhow!("could not create headset swapchain: {e}"))?;
        let images = swapchain
            .enumerate_images()
            .map_err(|e| anyhow::anyhow!("could not get headset images: {e}"))?;
        let mut textures = Vec::with_capacity(images.len());
        let mut views = Vec::with_capacity(images.len());
        for raw in images {
            let borrowed = unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&raw) }
                .ok_or_else(|| anyhow::anyhow!("OpenXR returned a null swapchain image"))?;
            let resource = borrowed.clone();
            let hal_texture = unsafe {
                wgpu::hal::dx12::Device::texture_from_raw(
                    resource,
                    texture_format,
                    wgpu::TextureDimension::D2,
                    wgpu::Extent3d { width: extent[0], height: extent[1], depth_or_array_layers: 2 },
                    1,
                    1,
                )
            };
            let desc = wgpu::TextureDescriptor {
                label: Some("OpenXR headset image"),
                size: wgpu::Extent3d { width: extent[0], height: extent[1], depth_or_array_layers: 2 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: texture_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            };
            let texture = unsafe { device.create_texture_from_hal::<wgpu::hal::api::Dx12>(hal_texture, &desc, wgpu::TextureUses::UNINITIALIZED) };
            let eye_views = std::array::from_fn(|eye| texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some(if eye == 0 { "OpenXR left eye" } else { "OpenXR right eye" }),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: eye as u32,
                array_layer_count: Some(1),
                ..Default::default()
            }));
            textures.push(texture);
            views.push(eye_views);
        }
        let runtime_name = instance.properties().map(|p| p.runtime_name).unwrap_or_else(|_| "OpenXR runtime".into());
        log::info!("OpenXR ready: {runtime_name}, {} × {} per eye", extent[0], extent[1]);
        Ok(Self {
            _entry: entry,
            instance,
            system,
            session,
            frame_wait,
            frame_stream,
            space,
            swapchain,
            textures,
            views,
            extent,
            format: texture_format,
            blend,
            running: false,
            active_frame: None,
            tracking_origin: None,
            error: None,
        })
    }

    pub fn extent(&self) -> [u32; 2] { self.extent }
    pub fn format(&self) -> wgpu::TextureFormat { self.format }
    pub fn error(&self) -> Option<&str> { self.error.as_deref() }

    pub fn poll_begin(&mut self) -> anyhow::Result<Option<XrFrame>> {
        let mut event = openxr::EventDataBuffer::new();
        while let Some(ev) = self.instance.poll_event(&mut event)? {
            if let openxr::Event::SessionStateChanged(s) = ev {
                match s.state() {
                    openxr::SessionState::READY => {
                        self.session.begin(openxr::ViewConfigurationType::PRIMARY_STEREO)?;
                        self.running = true;
                    }
                    openxr::SessionState::STOPPING => {
                        self.session.end()?;
                        self.running = false;
                    }
                    openxr::SessionState::EXITING | openxr::SessionState::LOSS_PENDING => {
                        self.running = false;
                    }
                    _ => {}
                }
            }
        }
        if !self.running {
            return Ok(None);
        }
        let state = self.frame_wait.wait()?;
        self.frame_stream.begin()?;
        let views = if state.should_render {
            self.session.locate_views(openxr::ViewConfigurationType::PRIMARY_STEREO, state.predicted_display_time, &self.space)?.1
        } else {
            Vec::new()
        };
        let image = if state.should_render {
            let i = self.swapchain.acquire_image()? as usize;
            self.swapchain.wait_image(openxr::Duration::INFINITE)?;
            Some(i)
        } else {
            None
        };
        let frame = ActiveXrFrame { display_time: state.predicted_display_time, views, image, should_render: state.should_render };
        let mut tracked_views = frame.views.clone();
        if let [left, right] = tracked_views.as_mut_slice() {
            let mid = glam::Vec3::new(
                (left.pose.position.x + right.pose.position.x) * 0.5,
                (left.pose.position.y + right.pose.position.y) * 0.5,
                (left.pose.position.z + right.pose.position.z) * 0.5,
            );
            let q0 = glam::Quat::from_xyzw(left.pose.orientation.x, left.pose.orientation.y, left.pose.orientation.z, left.pose.orientation.w).normalize();
            let (origin_q, origin_p) = *self.tracking_origin.get_or_insert((q0, mid));
            for view in tracked_views.iter_mut() {
                let q = glam::Quat::from_xyzw(view.pose.orientation.x, view.pose.orientation.y, view.pose.orientation.z, view.pose.orientation.w).normalize();
                let p = glam::Vec3::new(view.pose.position.x, view.pose.position.y, view.pose.position.z);
                let q = origin_q.conjugate() * q;
                let p = origin_q.conjugate() * (p - origin_p);
                view.pose.orientation = openxr::Quaternionf { x: q.x, y: q.y, z: q.z, w: q.w };
                view.pose.position = openxr::Vector3f { x: p.x, y: p.y, z: p.z };
            }
        }
        let out = XrFrame { display_time: frame.display_time, views: tracked_views, image: frame.image, should_render: frame.should_render };
        self.active_frame = Some(frame);
        Ok(Some(out))
    }

    pub fn target(&self, eye: usize) -> Option<wgpu::TextureView> {
        let i = self.active_frame.as_ref()?.image?;
        let _keep_alive = self.views.get(i)?;
        let texture = self.textures.get(i)?;
        Some(texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(if eye == 0 { "OpenXR left eye" } else { "OpenXR right eye" }),
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: eye as u32,
            array_layer_count: Some(1),
            ..Default::default()
        }))
    }

    pub fn finish(&mut self) -> anyhow::Result<()> {
        let Some(frame) = self.active_frame.take() else { return Ok(()); };
        if let Some(_) = frame.image {
            self.swapchain.release_image()?;
        }
        if frame.should_render && frame.image.is_some() && frame.views.len() == 2 {
            let rect = openxr::Rect2Di { offset: openxr::Offset2Di { x: 0, y: 0 }, extent: openxr::Extent2Di { width: self.extent[0] as i32, height: self.extent[1] as i32 } };
            let projection_views = [0, 1].map(|eye| openxr::CompositionLayerProjectionView::new()
                .pose(frame.views[eye].pose)
                .fov(frame.views[eye].fov)
                .sub_image(openxr::SwapchainSubImage::new().swapchain(&self.swapchain).image_array_index(eye as u32).image_rect(rect)));
            let projection = openxr::CompositionLayerProjection::new().space(&self.space).views(&projection_views);
            self.frame_stream.end(frame.display_time, self.blend, &[&projection])?;
        } else {
            self.frame_stream.end(frame.display_time, self.blend, &[])?;
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

/// State for the experimental VR camera preview.
#[derive(Clone, Debug)]
pub struct VrSystem {
    pub enabled: bool,
    /// Interpupillary distance in metres (typically 0.058 - 0.068 m).
    pub ipd: f32,
    /// Vertical field of view in degrees.
    pub fov_v: f32,
    /// Horizontal field of view in degrees.
    pub fov_h: f32,
    /// Whether a likely OpenXR runtime installation was found on this machine.
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
    /// Creates preview state and looks for a likely OpenXR runtime installation.
    pub fn new(enabled: bool) -> Self {
        let (detected, name) = detect_openxr_runtime();
        Self {
            enabled,
            ipd: QUEST_2_IPD_PRESET_2,
            fov_v: QUEST_2_DEFAULT_FOV_V,
            fov_h: QUEST_2_DEFAULT_FOV_H,
            runtime_detected: detected,
            runtime_name: name,
            pose: HeadPose::default(),
            center_yaw: 0.0,
            center_pos: DVec3::ZERO,
        }
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

        // Apply mouse/head tracking fusion:
        self.pose.yaw = mouse_look.0 - self.center_yaw;
        self.pose.pitch = mouse_look.1;

        // Construct orientation quaternion from yaw/pitch/roll:
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

/// Looks for a likely OpenXR runtime installation. This is informational only;
/// the current preview does not connect to the runtime.
fn detect_openxr_runtime() -> (bool, String) {
    // Check known Oculus / Meta Quest PC paths:
    let oculus_link_path = Path::new(r"C:\Program Files\Oculus\Support\oculus-runtime\oculus_openxr_64.json");
    if oculus_link_path.exists() {
        return (true, "Meta Quest Link (Oculus OpenXR)".to_string());
    }

    // Check SteamVR OpenXR runtime manifest:
    let steamvr_openxr = Path::new(r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR\steamxr_win64.json");
    if steamvr_openxr.exists() {
        return (true, "SteamVR OpenXR (Quest / PC VR)".to_string());
    }

    // Check environment variable XR_RUNTIME_JSON
    if let Ok(xr_path) = std::env::var("XR_RUNTIME_JSON") {
        if Path::new(&xr_path).exists() {
            return (true, format!("Custom OpenXR Runtime: {xr_path}"));
        }
    }

    (false, "No OpenXR runtime detected (preview only)".to_string())
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

        // Recenter headset
        vr.recenter();
        assert!((vr.center_yaw - 1.5).abs() < 1e-6);

        // After recenter, new pose yaw relative to center
        vr.update((1.5, 0.4), 0.016);
        assert!((vr.pose.yaw - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_vr_eye_projection() {
        let vr = VrSystem::new(true);
        let proj = vr.eye_projection(Eye::Left, 1.0, 0.1);
        let cols = proj.to_cols_array();
        assert!(cols.iter().all(|v| v.is_finite()));
        assert!(cols[0] > 0.0); // positive x scaling
        assert!(cols[5] > 0.0); // positive y scaling
    }
}
