//! Fullscreen Vulkan presenter for XREAL glasses.
//!
//! This first version draws a side-by-side stereo *test pattern* on the glasses' output so we can check that the
//! glasses really show a different image to each eye in their SBS mode. It will grow into the thing that imports
//! SteamVR's per-eye textures and presents them.
//!
//! usage: xreal-presenter [--monitor NAME] [--reproject] [--sim-pose [--sim-yaw DEG] [--sim-pitch DEG] [--sim-pitch-amp DEG]] [--dump DIR [--dump-frames N]]      (default monitor name: DP-1)
//!
//! Left half of the screen = left eye (red tint), right half = right eye (blue tint). A green square slides across
//! each half; its position differs by a few pixels between the eyes, so in a working stereo mode it appears to
//! float at a different depth from the frame. White borders and a white centre line show the exact edges.

mod tracking;
mod warp;

use ash::{khr, vk, Device, Entry, Instance};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

/// Sync file for the GPU work still writing this dma-buf (DMA_BUF_IOCTL_EXPORT_SYNC_FILE), or None if the kernel can't export it.
fn export_write_fence(dmabuf_fd: RawFd) -> Option<RawFd> {
    #[repr(C)]
    struct ExportSyncFile { flags: u32, fd: i32 }
    const DMA_BUF_IOCTL_EXPORT_SYNC_FILE: libc::c_ulong = 0xC008_6202; // _IOWR('b', 2, struct dma_buf_export_sync_file)
    let mut req = ExportSyncFile { flags: 1, fd: -1 }; // DMA_BUF_SYNC_READ: wait for the writers
    let ok = unsafe { libc::ioctl(dmabuf_fd, DMA_BUF_IOCTL_EXPORT_SYNC_FILE, &mut req) } == 0;
    (ok && req.fd >= 0).then_some(req.fd)
}

/// Wait up to `timeout_ms` for a sync file to signal; returns whether it is still pending. 0 only checks.
fn sync_file_pending(fd: RawFd, timeout_ms: i32) -> bool {
    let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, timeout_ms) == 0 }
}

/// fd of the connection to the driver (or -1); lets the render thread send vsync messages.
static DRIVER_FD: AtomicI32 = AtomicI32::new(-1);

// ---- link to the SteamVR driver -------------------------------------------------------------------------------
// The driver (driver/src/xreal_driver.cpp) connects to a SOCK_SEQPACKET unix socket and sends fixed 16-word messages:
//   1 SET     [1, set_id, width, height, vk_format, usage, create_flags] + 3 fds (SCM_RIGHTS)
//   2 DESTROY [2, set_id]
//   3 PRESENT [3, left_set, left_index, right_set, right_index, frame_number, render_qw, qx, qy, qz as f32 bits]

struct ImportedImage {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,      // only with --reproject
    set: vk::DescriptorSet,   // only with --reproject
}

struct SetInfo {
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    flags: u32,
    fds: [RawFd; 3],
    sync_fds: [RawFd; 3],               // our own dups of the dma-buf fds, used only to ask the kernel about the writer's fences
    images: Option<Vec<ImportedImage>>, // imported lazily, on first use
}

#[derive(Clone, Copy)]
struct PresentMsg {
    left: (u32, u32),
    right: (u32, u32),
    frame: u32,
    at: Instant,
    render_q: Option<[f32; 4]>, // head orientation SteamVR rendered this frame for (w, x, y, z)
    bounds: [[f32; 4]; 2],       // per eye: valid region of the texture (umin, vmin, umax, vmax)
}

#[derive(Default)]
struct Shared {
    sets: HashMap<u32, SetInfo>,
    destroyed: Vec<u32>,
    present: Option<PresentMsg>,
    connected: bool,
}

fn link_thread(shared: Arc<Mutex<Shared>>, pose: Arc<Mutex<tracking::PoseState>>) {
    unsafe {
        // Abstract socket (leading NUL): visible from SteamVR's pressure-vessel container, unlike a file path.
        let name = format!("xreal-presenter-{}", libc::getuid());
        let path = format!("@{name}");
        let ls = libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0);
        let mut addr: libc::sockaddr_un = std::mem::zeroed();
        addr.sun_family = libc::AF_UNIX as _;
        for (i, b) in name.bytes().enumerate() {
            addr.sun_path[i + 1] = b as _;
        }
        let len = (std::mem::size_of::<libc::sa_family_t>() + 1 + name.len()) as u32;
        if libc::bind(ls, &addr as *const _ as *const libc::sockaddr, len) != 0 || libc::listen(ls, 1) != 0 {
            eprintln!("cannot listen on {path}: {}", std::io::Error::last_os_error());
            return;
        }
        println!("waiting for the driver on {path}");
        loop {
            let c = libc::accept(ls, std::ptr::null_mut(), std::ptr::null_mut());
            if c < 0 {
                continue;
            }
            // Abstract sockets have no file permissions, so only accept a peer running as our own user.
            let mut cred: libc::ucred = std::mem::zeroed();
            let mut clen = std::mem::size_of::<libc::ucred>() as u32;
            if libc::getsockopt(c, libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut _, &mut clen) != 0
                || cred.uid != libc::getuid()
            {
                eprintln!("rejected a connection from another user");
                libc::close(c);
                continue;
            }
            println!("driver connected (pid {})", cred.pid);
            DRIVER_FD.store(c, Ordering::Relaxed);
            shared.lock().unwrap().connected = true;
            // Stream the head pose to the driver (type 4: [4, ts_lo, ts_hi, w, x, y, z as f32 bits, valid, wx, wy, wz as f32 bits, host_ns_lo, host_ns_hi]).
            let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
            {
                let (alive, pose) = (alive.clone(), pose.clone());
                std::thread::spawn(move || {
                    let (mut last_sample, mut last_valid, mut last_sent) = (0u64, false, Instant::now());
                    while alive.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        let p = *pose.lock().unwrap();
                        // Only new samples (or a change of validity), plus an occasional repeat so the driver sees the link is alive.
                        if p.host_ns == last_sample && p.valid == last_valid && last_sent.elapsed() < std::time::Duration::from_millis(50) {
                            continue;
                        }
                        (last_sample, last_valid, last_sent) = (p.host_ns, p.valid, Instant::now());
                        let w: [u32; 16] = [4, p.timestamp_ns as u32, (p.timestamp_ns >> 32) as u32, p.q[0].to_bits(), p.q[1].to_bits(),
                                            p.q[2].to_bits(), p.q[3].to_bits(), p.valid as u32, p.omega[0].to_bits(), p.omega[1].to_bits(), p.omega[2].to_bits(),
                                            p.host_ns as u32, (p.host_ns >> 32) as u32, 0, 0, 0];
                        let n = unsafe { libc::send(c, w.as_ptr() as *const _, 64, libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT) };
                        if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
                            break;
                        }
                    }
                });
            }
            loop {
                let mut words = [0u32; 16];
                let mut ctl = [0u64; 8]; // aligned control buffer
                let mut iov = libc::iovec { iov_base: words.as_mut_ptr() as *mut _, iov_len: 64 };
                let mut mh: libc::msghdr = std::mem::zeroed();
                mh.msg_iov = &mut iov;
                mh.msg_iovlen = 1;
                mh.msg_control = ctl.as_mut_ptr() as *mut _;
                mh.msg_controllen = std::mem::size_of_val(&ctl) as _;
                let n = libc::recvmsg(c, &mut mh, libc::MSG_CMSG_CLOEXEC);
                let mut fds: Vec<RawFd> = vec![];
                let mut cm = libc::CMSG_FIRSTHDR(&mh);
                while !cm.is_null() {
                    if (*cm).cmsg_level == libc::SOL_SOCKET && (*cm).cmsg_type == libc::SCM_RIGHTS {
                        let len = ((*cm).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / 4;
                        let data = libc::CMSG_DATA(cm) as *const RawFd;
                        for i in 0..len {
                            fds.push(*data.add(i));
                        }
                    }
                    cm = libc::CMSG_NXTHDR(&mh, cm);
                }
                if n <= 0 {
                    for fd in fds {
                        libc::close(fd);
                    }
                    break;
                }
                let mut sh = shared.lock().unwrap();
                match words[0] {
                    1 if fds.len() == 3 => {
                        sh.sets.insert(
                            words[1],
                            SetInfo { width: words[2], height: words[3], format: words[4], usage: words[5], flags: words[6],
                                      fds: [fds[0], fds[1], fds[2]], sync_fds: [libc::dup(fds[0]), libc::dup(fds[1]), libc::dup(fds[2])], images: None },
                        );
                    }
                    2 => sh.destroyed.push(words[1]),
                    3 => {
                        let q = [f32::from_bits(words[6]), f32::from_bits(words[7]), f32::from_bits(words[8]), f32::from_bits(words[9])];
                        let render_q = if q.iter().all(|v| v.is_finite()) && q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3] > 0.5 { Some(q) } else { None };
                        let unpack = |lo: u32, hi: u32| [(lo & 0xffff) as f32 / 65535.0, (lo >> 16) as f32 / 65535.0, (hi & 0xffff) as f32 / 65535.0, (hi >> 16) as f32 / 65535.0];
                        let mut bounds = [unpack(words[10], words[11]), unpack(words[12], words[13])];
                        for b in bounds.iter_mut() {
                            if b[0] == b[2] && b[1] == b[3] { *b = [0.0, 0.0, 1.0, 1.0]; } // older driver or empty: whole texture
                        }
                        sh.present = Some(PresentMsg { left: (words[1], words[2]), right: (words[3], words[4]), frame: words[5], at: Instant::now(), render_q, bounds });
                    }
                    _ => {
                        for fd in fds {
                            libc::close(fd);
                        }
                    }
                }
            }
            alive.store(false, std::sync::atomic::Ordering::Relaxed);
            DRIVER_FD.store(-1, Ordering::Relaxed);
            libc::close(c);
            println!("driver disconnected");
            let mut sh = shared.lock().unwrap();
            let ids: Vec<u32> = sh.sets.keys().copied().collect();
            sh.destroyed.extend(ids);
            sh.present = None;
            sh.connected = false;
        }
    }
}

/// Everything the reprojection pass needs (created only with --reproject).
struct WarpPipe {
    sampler: vk::Sampler,
    desc_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

/// Debug capture of what the glasses show (the centre of each eye's half), copied by the frame's own submission so that
/// capturing does not change the timing being captured.
struct Capture {
    buf: vk::Buffer,
    mem: vk::DeviceMemory,
    ptr: *const u8,
    w: u32,
    h: u32,
    tx: std::sync::mpsc::Sender<(u32, Vec<u8>)>,
}

/// The two eye images to show, with what reprojection needs.
struct Eyes {
    imgs: [(vk::Image, u32, u32); 2],
    sets: [vk::DescriptorSet; 2],
    render_q: Option<[f32; 4]>,
    bounds: [[f32; 4]; 2],
    frame: u32,            // SteamVR's present counter for this frame
    slot: (u32, u32),      // left eye (set id, index)
    age_ms: f32,           // how old the chosen frame was
    fence_supported: bool, // the kernel could export the writer's fence
    fence_pending: bool,   // the writer had not finished when we looked
    wait_fds: [RawFd; 2],  // per eye: the writer's sync file for the GPU to wait on, or -1
}

struct Gfx {
    _entry: Entry,
    instance: Instance,
    surface_loader: khr::surface::Instance,
    surface: vk::SurfaceKHR,
    phys: vk::PhysicalDevice,
    device: Device,
    queue: vk::Queue,
    queue_family: u32,
    swapchain_loader: khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    format: vk::Format,
    extent: vk::Extent2D,
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    image_available: vk::Semaphore,
    render_done: Vec<vk::Semaphore>,
    in_flight: vk::Fence,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    seen: std::collections::HashSet<vk::Image>, // imported images we have already transitioned once
    vsync_seq: u32,
    warp: Option<WarpPipe>,
    reproject: bool,
    dump_dir: Option<std::path::PathBuf>,
    dump_remaining: u32,
    dump_count: u32,
    dump_index: u32,
    capture: Option<Capture>,
    capture_pending: Option<u32>, // dump index copied by the submission in flight
    pub fallbacks: u32,  // frames drawn as the test pattern while the driver was connected (should stay 0)
    last_delta_deg: f32, // head rotation between SteamVR's render pose and now, last warped frame
    semaphore_fd: Option<khr::external_semaphore_fd::Device>, // None: the GPU cannot wait on sync files, so the CPU waits
    read_ready: [vk::Semaphore; 2], // per eye: carries the writer's sync file into our submission
    present_wait: Option<khr::present_wait::Device>, // None: vblank times are estimated from acquire instead
    present_id: u64,              // id of our last present, 0 when there is none to wait for
    present_wait_timeouts: u32,   // consecutive timeouts; the compositor may not report presentation
    present_wait_active: bool,    // false: vblank is estimated from acquire, and present wait is retried at `present_wait_retry`
    present_wait_retry: std::time::Instant,
    used_frame: Option<(RawFd, u32)>, // driver connection and SteamVR frame last reported as in use
    pub new_frames: u32,          // SteamVR frames shown for the first time since the last report
    pub new_frame_age_ms: (f32, f32), // sum and max of their age when picked up, since the last report
    last_render_q: Option<[f32; 4]>,
    pub render_steps_deg: Vec<f32>, // how far SteamVR's render pose moved between consecutive new frames, since the last report
}

impl Gfx {
    unsafe fn new(window: &Window, reproject: bool) -> Result<Gfx, Box<dyn std::error::Error>> {
        let entry = Entry::load()?;
        let display = window.display_handle()?.as_raw();
        let win = window.window_handle()?.as_raw();
        let exts = ash_window::enumerate_required_extensions(display)?;
        let app = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 3, 0));
        let instance = entry.create_instance(
            &vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(exts),
            None,
        )?;
        let surface = ash_window::create_surface(&entry, &instance, display, win, None)?;
        let surface_loader = khr::surface::Instance::new(&entry, &instance);

        let (phys, queue_family) = instance
            .enumerate_physical_devices()?
            .into_iter()
            .find_map(|p| {
                instance
                    .get_physical_device_queue_family_properties(p)
                    .iter()
                    .enumerate()
                    .find(|(i, q)| {
                        q.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                            && surface_loader.get_physical_device_surface_support(p, *i as u32, surface).unwrap_or(false)
                    })
                    .map(|(i, _)| (p, i as u32))
            })
            .ok_or("no Vulkan device can present to this surface")?;
        let props = instance.get_physical_device_properties(phys);
        println!("GPU: {}", CStr::from_ptr(props.device_name.as_ptr()).to_string_lossy());

        let prio = [1.0f32];
        let qci = [vk::DeviceQueueCreateInfo::default().queue_family_index(queue_family).queue_priorities(&prio)];
        let available = instance.enumerate_device_extension_properties(phys)?;
        let has_ext = |name: &CStr| available.iter().any(|e| e.extension_name_as_c_str() == Ok(name));
        let has_semaphore_fd = has_ext(khr::external_semaphore_fd::NAME);
        let mut sem_props = vk::ExternalSemaphoreProperties::default();
        instance.get_physical_device_external_semaphore_properties(
            phys,
            &vk::PhysicalDeviceExternalSemaphoreInfo::default().handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD),
            &mut sem_props,
        );
        let gpu_waits = has_semaphore_fd && sem_props.external_semaphore_features.contains(vk::ExternalSemaphoreFeatureFlags::IMPORTABLE);
        println!("waiting for SteamVR's writes on the {}", if gpu_waits { "GPU" } else { "CPU (no sync-file semaphore import)" });
        let mut dev_exts = vec![khr::swapchain::NAME.as_ptr(), khr::external_memory_fd::NAME.as_ptr()];
        if gpu_waits { dev_exts.push(khr::external_semaphore_fd::NAME.as_ptr()); }
        let (mut id_feat, mut wait_feat) = (vk::PhysicalDevicePresentIdFeaturesKHR::default(), vk::PhysicalDevicePresentWaitFeaturesKHR::default());
        if has_ext(khr::present_id::NAME) && has_ext(khr::present_wait::NAME) {
            let mut f2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut id_feat).push_next(&mut wait_feat);
            instance.get_physical_device_features2(phys, &mut f2);
        }
        let present_waits = id_feat.present_id == vk::TRUE && wait_feat.present_wait == vk::TRUE;
        println!("vblank times from {}", if present_waits { "present wait" } else { "acquire (no present wait)" });
        if present_waits { dev_exts.extend([khr::present_id::NAME.as_ptr(), khr::present_wait::NAME.as_ptr()]); }
        let mut f13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let mut id_on = vk::PhysicalDevicePresentIdFeaturesKHR::default().present_id(true);
        let mut wait_on = vk::PhysicalDevicePresentWaitFeaturesKHR::default().present_wait(true);
        let mut dci = vk::DeviceCreateInfo::default().queue_create_infos(&qci).enabled_extension_names(&dev_exts).push_next(&mut f13);
        if present_waits { dci = dci.push_next(&mut id_on).push_next(&mut wait_on); }
        let device = instance.create_device(phys, &dci, None)?;
        let mem_props = instance.get_physical_device_memory_properties(phys);
        let queue = device.get_device_queue(queue_family, 0);
        let swapchain_loader = khr::swapchain::Device::new(&instance, &device);
        let pool = device.create_command_pool(
            &vk::CommandPoolCreateInfo::default()
                .queue_family_index(queue_family)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
            None,
        )?;
        let cmd = device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default().command_pool(pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1),
        )?[0];
        let image_available = device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?;
        let in_flight = device.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None)?;
        let read_ready = [
            device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
            device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
        ];
        let semaphore_fd = gpu_waits.then(|| khr::external_semaphore_fd::Device::new(&instance, &device));
        let present_wait = present_waits.then(|| khr::present_wait::Device::new(&instance, &device));

        let mut g = Gfx {
            _entry: entry, instance, surface_loader, surface, phys, device, queue, queue_family, swapchain_loader,
            swapchain: vk::SwapchainKHR::null(), images: vec![], views: vec![], format: vk::Format::B8G8R8A8_UNORM,
            extent: vk::Extent2D { width: 1, height: 1 }, pool, cmd, image_available, render_done: vec![], in_flight,
            mem_props, seen: Default::default(), vsync_seq: 0, warp: None, reproject, dump_dir: None, dump_remaining: 0, dump_count: 30, dump_index: 0, capture: None, capture_pending: None, fallbacks: 0, last_delta_deg: 0.0,
            semaphore_fd, read_ready, present_wait, present_id: 0, present_wait_timeouts: 0, present_wait_active: true, present_wait_retry: std::time::Instant::now(), used_frame: None,
            new_frames: 0, new_frame_age_ms: (0.0, 0.0), last_render_q: None, render_steps_deg: vec![],
        };
        g.create_swapchain(window.inner_size())?;
        if reproject {
            g.warp = Some(g.create_warp()?);
            println!("reprojection enabled");
        }
        Ok(g)
    }

    unsafe fn create_swapchain(&mut self, size: PhysicalSize<u32>) -> Result<(), Box<dyn std::error::Error>> {
        self.device.device_wait_idle()?;
        for v in self.views.drain(..) {
            self.device.destroy_image_view(v, None);
        }
        for s in self.render_done.drain(..) {
            self.device.destroy_semaphore(s, None);
        }
        let caps = self.surface_loader.get_physical_device_surface_capabilities(self.phys, self.surface)?;
        let formats = self.surface_loader.get_physical_device_surface_formats(self.phys, self.surface)?;
        let fmt = formats
            .iter()
            .find(|f| f.format == vk::Format::B8G8R8A8_SRGB)
            .copied()
            .unwrap_or(formats[0]);
        self.format = fmt.format;
        self.extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D { width: size.width.max(1), height: size.height.max(1) }
        };
        let count = (caps.min_image_count + 1).min(if caps.max_image_count == 0 { u32::MAX } else { caps.max_image_count });
        let old = self.swapchain;
        self.present_id = 0;
        self.swapchain = self.swapchain_loader.create_swapchain(
            &vk::SwapchainCreateInfoKHR::default()
                .surface(self.surface)
                .min_image_count(count)
                .image_format(fmt.format)
                .image_color_space(fmt.color_space)
                .image_extent(self.extent)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST
                    | (caps.supported_usage_flags & vk::ImageUsageFlags::TRANSFER_SRC))
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(caps.current_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(vk::PresentModeKHR::FIFO)
                .clipped(true)
                .old_swapchain(old),
            None,
        )?;
        if old != vk::SwapchainKHR::null() {
            self.swapchain_loader.destroy_swapchain(old, None);
        }
        self.images = self.swapchain_loader.get_swapchain_images(self.swapchain)?;
        for &img in &self.images {
            let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
            self.views.push(self.device.create_image_view(
                &vk::ImageViewCreateInfo::default().image(img).view_type(vk::ImageViewType::TYPE_2D).format(self.format).subresource_range(range),
                None,
            )?);
            self.render_done.push(self.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?);
        }
        println!("swapchain: {}x{} {:?}, {} images", self.extent.width, self.extent.height, self.format, self.images.len());
        Ok(())
    }


    /// Test pattern: used until a driver is connected and presenting.
    unsafe fn record_pattern(&self, cmd: vk::CommandBuffer, idx: usize, image: vk::Image, t: f32) {
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_attachment = vk::ImageMemoryBarrier::default()
            .image(image).subresource_range(range)
            .old_layout(vk::ImageLayout::UNDEFINED).new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(), &[], &[], &[to_attachment]);

        let (w, h) = (self.extent.width as i32, self.extent.height as i32);
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(self.views[idx]).image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR).store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } })];
        self.device.cmd_begin_rendering(cmd, &vk::RenderingInfo::default()
            .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent: self.extent }).layer_count(1).color_attachments(&att));

        let rect = |x: i32, y: i32, rw: i32, rh: i32, c: [f32; 4]| {
            let (x, y) = (x.clamp(0, w), y.clamp(0, h));
            let (rw, rh) = (rw.min(w - x).max(0), rh.min(h - y).max(0));
            if rw == 0 || rh == 0 { return; }
            self.device.cmd_clear_attachments(cmd,
                &[vk::ClearAttachment { aspect_mask: vk::ImageAspectFlags::COLOR, color_attachment: 0, clear_value: vk::ClearValue { color: vk::ClearColorValue { float32: c } } }],
                &[vk::ClearRect { rect: vk::Rect2D { offset: vk::Offset2D { x, y }, extent: vk::Extent2D { width: rw as u32, height: rh as u32 } }, base_array_layer: 0, layer_count: 1 }]);
        };
        let half = w / 2;
        let white = [1.0, 1.0, 1.0, 1.0];
        for (eye, (x0, tint)) in [(0, [0.30, 0.0, 0.0, 1.0]), (half, [0.0, 0.0, 0.30, 1.0])].into_iter().enumerate() {
            rect(x0, 0, half, h, tint);
            let b = 8;
            rect(x0, 0, half, b, white);              // top
            rect(x0, h - b, half, b, white);          // bottom
            rect(x0, 0, b, h, white);                 // left
            rect(x0 + half - b, 0, b, h, white);      // right
            // cross-hair at the centre of the eye
            rect(x0 + half / 2 - 1, h / 2 - 60, 3, 120, white);
            rect(x0 + half / 2 - 60, h / 2 - 1, 120, 3, white);
            // moving square; offset differs per eye to create disparity
            let travel = (half - 220) as f32;
            let pos = ((t * 150.0) % (2.0 * travel)).abs();
            let pos = if pos > travel { 2.0 * travel - pos } else { pos };
            let disparity = if eye == 0 { 24 } else { -24 };
            rect(x0 + 100 + pos as i32 + disparity, h / 2 + 150, 100, 100, [0.0, 1.0, 0.0, 1.0]);
        }
        rect(half - 2, 0, 4, h, white); // centre line (the boundary between the two eyes)

        self.device.cmd_end_rendering(cmd);
        let to_present = vk::ImageMemoryBarrier::default()
            .image(image).subresource_range(range)
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL).new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(), &[], &[], &[to_present]);
    }

    /// Blit each eye's SteamVR image into its half of the swapchain image.
    unsafe fn record_blit(&mut self, cmd: vk::CommandBuffer, dst: vk::Image, eyes: [(vk::Image, u32, u32); 2], bounds: [[f32; 4]; 2]) {
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_dst = vk::ImageMemoryBarrier::default()
            .image(dst).subresource_range(range)
            .old_layout(vk::ImageLayout::UNDEFINED).new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(), &[], &[], &[to_dst]);
        let (w, h) = (self.extent.width as i32, self.extent.height as i32);
        let half = w / 2;
        for (eye, (img, sw, sh)) in eyes.into_iter().enumerate() {
            let old = if self.seen.insert(img) { vk::ImageLayout::UNDEFINED } else { vk::ImageLayout::TRANSFER_SRC_OPTIMAL };
            let to_src = vk::ImageMemoryBarrier::default()
                .image(img).subresource_range(range)
                .old_layout(old).new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE).dst_access_mask(vk::AccessFlags::TRANSFER_READ);
            self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(), &[], &[], &[to_src]);
            let layers = vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1);
            let x0 = if eye == 0 { 0 } else { half };
            let b = bounds[eye];
            let (sx0, sx1) = ((b[0] * sw as f32).round() as i32, (b[2] * sw as f32).round() as i32);
            let (sy0, sy1) = ((b[1] * sh as f32).round() as i32, (b[3] * sh as f32).round() as i32);
            let blit = vk::ImageBlit::default()
                .src_subresource(layers).src_offsets([vk::Offset3D { x: sx0, y: sy0, z: 0 }, vk::Offset3D { x: sx1, y: sy1, z: 1 }])
                .dst_subresource(layers).dst_offsets([vk::Offset3D { x: x0, y: 0, z: 0 }, vk::Offset3D { x: x0 + half, y: h, z: 1 }]);
            self.device.cmd_blit_image(cmd, img, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, dst, vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[blit], vk::Filter::LINEAR);
        }
        let to_present = vk::ImageMemoryBarrier::default()
            .image(dst).subresource_range(range)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL).new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(), &[], &[], &[to_present]);
    }

    /// Sampler, descriptor pool/layout and the graphics pipeline for the reprojection pass.
    unsafe fn create_warp(&self) -> Result<WarpPipe, Box<dyn std::error::Error>> {
        let spv = |bytes: &[u8]| ash::util::read_spv(&mut std::io::Cursor::new(bytes));
        let vs_code = spv(include_bytes!("../shaders/warp.vert.spv"))?;
        let fs_code = spv(include_bytes!("../shaders/warp.frag.spv"))?;
        let vs = self.device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&vs_code), None)?;
        let fs = self.device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&fs_code), None)?;
        let sampler = self.device.create_sampler(
            &vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR).min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                .border_color(vk::BorderColor::FLOAT_OPAQUE_BLACK)
                .max_lod(0.0),
            None,
        )?;
        let binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let desc_layout = self.device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding), None)?;
        let sizes = [vk::DescriptorPoolSize::default().ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(64)];
        let pool = self.device.create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo::default().max_sets(64).pool_sizes(&sizes)
                .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET),
            None,
        )?;
        let layouts = [desc_layout];
        let ranges = [vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::FRAGMENT).offset(0).size(80)];
        let layout = self.device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts).push_constant_ranges(&ranges), None)?;

        let entry = CStr::from_bytes_with_nul(b"main\0")?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vs).name(entry),
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(fs).name(entry),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blend_att = [vk::PipelineColorBlendAttachmentState::default().color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_att);
        let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
        let formats = [self.format];
        let mut rendering = vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let ci = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages).vertex_input_state(&vertex_input).input_assembly_state(&input_assembly)
            .viewport_state(&viewport).rasterization_state(&raster).multisample_state(&multisample)
            .color_blend_state(&blend).dynamic_state(&dynamic).layout(layout).push_next(&mut rendering);
        let pipeline = self.device.create_graphics_pipelines(vk::PipelineCache::null(), &[ci], None).map_err(|e| e.1)?[0];
        self.device.destroy_shader_module(vs, None);
        self.device.destroy_shader_module(fs, None);
        Ok(WarpPipe { sampler, desc_layout, pool, layout, pipeline })
    }

    /// Reprojection pass: draw each eye by sampling its image along the lines of sight the head has turned to since
    /// SteamVR rendered it. `rows` is the rotation (three vec4 rows) from warp::push_rows.
    unsafe fn record_warp(&mut self, cmd: vk::CommandBuffer, idx: usize, dst: vk::Image, e: &Eyes, rows: [f32; 12]) {
        let (pipeline, layout) = {
            let w = self.warp.as_ref().unwrap();
            (w.pipeline, w.layout)
        };
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_attachment = vk::ImageMemoryBarrier::default()
            .image(dst).subresource_range(range)
            .old_layout(vk::ImageLayout::UNDEFINED).new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(), &[], &[], &[to_attachment]);
        for &(img, _, _) in &e.imgs {
            let old = if self.seen.insert(img) { vk::ImageLayout::UNDEFINED } else { vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL };
            let to_read = vk::ImageMemoryBarrier::default()
                .image(img).subresource_range(range)
                .old_layout(old).new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE).dst_access_mask(vk::AccessFlags::SHADER_READ);
            self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(), &[], &[], &[to_read]);
        }
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(self.views[idx]).image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR).store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } })];
        self.device.cmd_begin_rendering(cmd, &vk::RenderingInfo::default()
            .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent: self.extent }).layer_count(1).color_attachments(&att));
        self.device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
        let (w, h) = (self.extent.width, self.extent.height);
        let half = w / 2;
        let mut push = [0f32; 20];
        push[..12].copy_from_slice(&rows);
        push[12..16].copy_from_slice(&warp::FOV);
        for eye in 0..2usize {
            push[16..20].copy_from_slice(&e.bounds[eye]);
            let bytes = std::slice::from_raw_parts(push.as_ptr() as *const u8, 80);
            let x = (eye as u32 * half) as i32;
            self.device.cmd_set_viewport(cmd, 0, &[vk::Viewport { x: x as f32, y: 0.0, width: half as f32, height: h as f32, min_depth: 0.0, max_depth: 1.0 }]);
            self.device.cmd_set_scissor(cmd, 0, &[vk::Rect2D { offset: vk::Offset2D { x, y: 0 }, extent: vk::Extent2D { width: half, height: h } }]);
            self.device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, layout, 0, &[e.sets[eye]], &[]);
            self.device.cmd_push_constants(cmd, layout, vk::ShaderStageFlags::FRAGMENT, 0, bytes);
            self.device.cmd_draw(cmd, 3, 1, 0, 0);
        }
        self.device.cmd_end_rendering(cmd);
        let to_present = vk::ImageMemoryBarrier::default()
            .image(dst).subresource_range(range)
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL).new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(), &[], &[], &[to_present]);
    }

    /// Persistently mapped buffer for both eyes' crops, and a thread that writes each captured frame as raw RGBA into `dir`.
    unsafe fn create_capture(&self, dir: &std::path::Path) -> Result<Capture, Box<dyn std::error::Error>> {
        let (w, h) = ((self.extent.width / 2).min(640), self.extent.height.min(400));
        let eye_bytes = (w * h * 4) as usize;
        let buf = self.device.create_buffer(&vk::BufferCreateInfo::default().size(2 * eye_bytes as u64).usage(vk::BufferUsageFlags::TRANSFER_DST), None)?;
        let reqs = self.device.get_buffer_memory_requirements(buf);
        let want = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let mt = (0..self.mem_props.memory_type_count)
            .find(|&j| reqs.memory_type_bits & (1 << j) != 0 && self.mem_props.memory_types[j as usize].property_flags.contains(want))
            .ok_or("no host-visible memory type")?;
        let mem = self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(reqs.size).memory_type_index(mt), None)?;
        self.device.bind_buffer_memory(buf, mem, 0)?;
        let ptr = self.device.map_memory(mem, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())? as *const u8;
        let bgra = matches!(self.format, vk::Format::B8G8R8A8_SRGB | vk::Format::B8G8R8A8_UNORM);
        let (tx, rx) = std::sync::mpsc::channel::<(u32, Vec<u8>)>();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            for (index, mut bytes) in rx {
                if bgra { for px in bytes.chunks_exact_mut(4) { px.swap(0, 2); } }
                for (k, tag) in ["L", "R"].into_iter().enumerate() {
                    if let Err(err) = std::fs::write(dir.join(format!("frame_{index:04}_{tag}_{w}x{h}.rgba")), &bytes[k * eye_bytes..(k + 1) * eye_bytes]) {
                        eprintln!("capture write failed: {err}");
                    }
                }
            }
        });
        Ok(Capture { buf, mem, ptr, w, h, tx })
    }

    /// Copy the centre of each eye's half of the finished swapchain image into the capture buffer.
    unsafe fn record_capture(&self, cmd: vk::CommandBuffer, image: vk::Image) {
        let Some(c) = &self.capture else { return };
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_src = vk::ImageMemoryBarrier::default().image(image).subresource_range(range)
            .old_layout(vk::ImageLayout::PRESENT_SRC_KHR).new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::TRANSFER_WRITE).dst_access_mask(vk::AccessFlags::TRANSFER_READ);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[to_src]);
        let half = self.extent.width / 2;
        let regions: Vec<vk::BufferImageCopy> = (0..2u32).map(|eye| vk::BufferImageCopy::default()
            .buffer_offset((eye * c.w * c.h * 4) as u64)
            .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
            .image_offset(vk::Offset3D { x: (eye * half + (half - c.w) / 2) as i32, y: ((self.extent.height - c.h) / 2) as i32, z: 0 })
            .image_extent(vk::Extent3D { width: c.w, height: c.h, depth: 1 })).collect();
        self.device.cmd_copy_image_to_buffer(cmd, image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, c.buf, &regions);
        let back = vk::ImageMemoryBarrier::default().image(image).subresource_range(range)
            .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL).new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .src_access_mask(vk::AccessFlags::TRANSFER_READ);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(), &[], &[], &[back]);
    }

    /// Free destroyed sets, import the sets the latest PRESENT refers to, and return the two eye images (if any).
    unsafe fn prepare_eyes(&mut self, shared: &Mutex<Shared>) -> Option<Eyes> {
        let mut guard = shared.lock().unwrap();
        let sh = &mut *guard;
        for id in sh.destroyed.drain(..) {
            if let Some(set) = sh.sets.remove(&id) {
                for fd in set.sync_fds { libc::close(fd); }
                match set.images {
                    Some(imgs) => {
                        for im in imgs {
                            self.seen.remove(&im.image);
                            if let Some(w) = &self.warp {
                                if im.view != vk::ImageView::null() { self.device.destroy_image_view(im.view, None); }
                                if im.set != vk::DescriptorSet::null() { let _ = self.device.free_descriptor_sets(w.pool, &[im.set]); }
                            }
                            self.device.destroy_image(im.image, None);
                            self.device.free_memory(im.memory, None);
                        }
                    }
                    None => {
                        for fd in set.fds {
                            libc::close(fd);
                        }
                    }
                }
            }
        }
        // Always the newest frame: SteamVR redraws a swap image two presents later, so an older one may be mid-draw when we read it.
        let p = sh.present?;
        let mut out = [(vk::Image::null(), 0u32, 0u32); 2];
        let mut sets = [vk::DescriptorSet::null(); 2];
        for (i, (sid, idx)) in [p.left, p.right].into_iter().enumerate() {
            let set = sh.sets.get_mut(&sid)?;
            if set.images.is_none() {
                match self.import_set(set) {
                    Ok(v) => {
                        println!("imported set {sid}: {}x{} format {}", set.width, set.height, set.format);
                        set.images = Some(v);
                    }
                    Err(e) => {
                        eprintln!("import of set {sid} failed: {e}");
                        sh.present = None;
                        return None;
                    }
                }
            }
            let img = &set.images.as_ref().unwrap()[idx as usize];
            out[i] = (img.image, set.width, set.height);
            sets[i] = img.set;
        }
        // Our previous read has finished (draw waited for it), so every older frame can go back to SteamVR (type 6: [6, frame]).
        let fd = DRIVER_FD.load(Ordering::Relaxed);
        if fd >= 0 && self.used_frame != Some((fd, p.frame)) {
            self.used_frame = Some((fd, p.frame));
            let age = p.at.elapsed().as_secs_f32() * 1000.0;
            self.new_frames += 1;
            self.new_frame_age_ms = (self.new_frame_age_ms.0 + age, self.new_frame_age_ms.1.max(age));
            if let (Some(a), Some(b)) = (self.last_render_q, p.render_q) {
                let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs().min(1.0);
                self.render_steps_deg.push(2.0 * d.acos().to_degrees());
            }
            self.last_render_q = p.render_q;
            let mut w = [0u32; 16];
            w[0] = 6;
            w[1] = p.frame;
            libc::send(fd, w.as_ptr() as *const _, 64, libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT);
        }
        // SteamVR calls Present when it has submitted its GPU work, not when that work has finished.
        let (mut fence_supported, mut fence_pending) = (false, false);
        let mut wait_fds = [-1; 2];
        for (eye, (sid, idx)) in [p.left, p.right].into_iter().enumerate() {
            let Some(fd) = sh.sets.get(&sid).and_then(|s| export_write_fence(s.sync_fds[idx as usize])) else { continue };
            fence_supported = true;
            fence_pending |= sync_file_pending(fd, 0);
            if self.semaphore_fd.is_some() {
                wait_fds[eye] = fd;
            } else {
                sync_file_pending(fd, 25);
                libc::close(fd);
            }
        }
        Some(Eyes { imgs: out, sets, render_q: p.render_q, bounds: p.bounds, frame: p.frame, slot: p.left, age_ms: p.at.elapsed().as_secs_f32() * 1000.0, fence_supported, fence_pending, wait_fds })
    }

    /// Make the next submission wait for SteamVR's writes; returns the semaphores to wait on. Takes ownership of the fds.
    unsafe fn import_wait_fds(&self, fds: [RawFd; 2]) -> Vec<vk::Semaphore> {
        let mut out = vec![];
        for (eye, fd) in fds.into_iter().enumerate() {
            if fd < 0 { continue; }
            let Some(loader) = &self.semaphore_fd else { libc::close(fd); continue };
            let info = vk::ImportSemaphoreFdInfoKHR::default()
                .semaphore(self.read_ready[eye]).flags(vk::SemaphoreImportFlags::TEMPORARY)
                .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD).fd(fd);
            match loader.import_semaphore_fd(&info) {
                Ok(()) => out.push(self.read_ready[eye]),
                Err(e) => {
                    eprintln!("sync file import failed: {e:?}");
                    libc::close(fd);
                }
            }
        }
        out
    }

    /// Import SteamVR's three swap textures. The parameters must match how SteamVR created the images (same format,
    /// size, usage, MUTABLE_FORMAT, optimal tiling) and the GPU must be the same one; see ALVR's driver for the
    /// reference. On success Vulkan owns the fds.
    unsafe fn import_set(&self, set: &SetInfo) -> Result<Vec<ImportedImage>, Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        for i in 0..3 {
            let mut ext = vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD);
            let ci = vk::ImageCreateInfo::default()
                .flags(vk::ImageCreateFlags::from_raw(set.flags))
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::from_raw(set.format as i32))
                .extent(vk::Extent3D { width: set.width, height: set.height, depth: 1 })
                .mip_levels(1).array_layers(1).samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::from_raw(set.usage))
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .push_next(&mut ext);
            let image = self.device.create_image(&ci, None)?;
            let reqs = self.device.get_image_memory_requirements(image);
            let mem_type = (0..self.mem_props.memory_type_count)
                .find(|&j| {
                    reqs.memory_type_bits & (1 << j) != 0
                        && self.mem_props.memory_types[j as usize].property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .ok_or("no device-local memory type for the imported image")?;
            let mut import = vk::ImportMemoryFdInfoKHR::default().handle_type(vk::ExternalMemoryHandleTypeFlags::OPAQUE_FD).fd(set.fds[i]);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let alloc = vk::MemoryAllocateInfo::default()
                .allocation_size(reqs.size).memory_type_index(mem_type)
                .push_next(&mut dedicated).push_next(&mut import);
            let memory = match self.device.allocate_memory(&alloc, None) {
                Ok(m) => m,
                Err(e) => {
                    self.device.destroy_image(image, None);
                    return Err(format!("allocate_memory with imported fd failed: {e:?}").into());
                }
            };
            self.device.bind_image_memory(image, memory, 0)?;
            let (view, dset) = if let Some(w) = &self.warp {
                let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
                let view = self.device.create_image_view(
                    &vk::ImageViewCreateInfo::default().image(image).view_type(vk::ImageViewType::TYPE_2D)
                        .format(vk::Format::from_raw(set.format as i32)).subresource_range(range),
                    None,
                )?;
                let layouts = [w.desc_layout];
                let dset = self.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(w.pool).set_layouts(&layouts))?[0];
                let info = [vk::DescriptorImageInfo::default().sampler(w.sampler).image_view(view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                self.device.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default().dst_set(dset).dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&info)],
                    &[],
                );
                (view, dset)
            } else {
                (vk::ImageView::null(), vk::DescriptorSet::null())
            };
            out.push(ImportedImage { image, memory, view, set: dset });
        }
        Ok(out)
    }

    /// Draw one frame: the driver's eyes if it is presenting, otherwise the stereo test pattern.
    unsafe fn draw(&mut self, t: f32, window: &Window, shared: &Mutex<Shared>, pose: &Mutex<tracking::PoseState>) -> Result<(), Box<dyn std::error::Error>> {
        self.device.wait_for_fences(&[self.in_flight], true, u64::MAX)?;
        if let (Some(index), Some(c)) = (self.capture_pending.take(), &self.capture) {
            let _ = c.tx.send((index, std::slice::from_raw_parts(c.ptr, (2 * c.w * c.h * 4) as usize).to_vec()));
        }
        // Waiting for our last frame to reach the display gives the vblank time and keeps only one frame queued.
        let mut vblank_ns = 0u64;
        // After a fallback the wait is retried every 5 s with a short timeout: the compositor often reports presentation
        // only once it is up, so a fallback at startup should not last for the whole session.
        let probing = !self.present_wait_active && std::time::Instant::now() >= self.present_wait_retry;
        let waited = match (&self.present_wait, self.present_id) {
            // While the wait is off the queue runs up to the swapchain's depth ahead of the display, so a probe asks about a
            // present from a few frames back: if that one reports, presentation feedback works.
            (Some(pw), id) if id > 0 && (self.present_wait_active || probing) => {
                if probing { Some(pw.wait_for_present(self.swapchain, id.saturating_sub(4).max(1), 20_000_000)) }
                else { Some(pw.wait_for_present(self.swapchain, id, 50_000_000)) }
            }
            _ => None,
        };
        match waited {
            Some(Ok(())) => {
                self.present_wait_timeouts = 0;
                if self.present_wait_active { vblank_ns = tracking::monotonic_ns(); }
                else {
                    println!("present wait works again; using it for vblank times");
                    self.present_wait_active = true;
                }
            }
            Some(Err(vk::Result::TIMEOUT)) => {
                self.present_wait_timeouts += 1;
                if probing {
                    self.present_wait_retry = std::time::Instant::now() + std::time::Duration::from_secs(5);
                } else if self.present_wait_timeouts >= 3 {
                    println!("present wait keeps timing out; estimating vblank from acquire instead (retrying every 5 s)");
                    self.present_wait_active = false;
                    self.present_wait_retry = std::time::Instant::now() + std::time::Duration::from_secs(5);
                }
            }
            Some(Err(vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::SUBOPTIMAL_KHR)) | None => {}
            Some(Err(e)) => return Err(e.into()),
        }
        let (idx, _) = match self.swapchain_loader.acquire_next_image(self.swapchain, u64::MAX, self.image_available, vk::Fence::null()) {
            Ok(r) => r,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return self.create_swapchain(window.inner_size()),
            Err(e) => return Err(e.into()),
        };
        self.device.reset_fences(&[self.in_flight])?;
        // Chosen after acquire, which can block for a refresh, so the frame is as fresh as possible when the GPU reads it.
        let eyes = self.prepare_eyes(shared);
        if eyes.is_none() && shared.lock().unwrap().connected {
            self.fallbacks += 1;
        }
        let mut capture_index = None;
        if let (Some(dir), Some(e)) = (self.dump_dir.clone(), &eyes) {
            // Debug: `touch DIR/trigger` captures the next frames as shown on the glasses (both eyes, centre crop) as raw RGBA.
            if self.dump_remaining == 0 && dir.join("trigger").exists() {
                let _ = std::fs::remove_file(dir.join("trigger"));
                self.dump_remaining = self.dump_count;
                println!("capturing {} frames to {}", self.dump_count, dir.display());
            }
            if self.dump_remaining > 0 && self.capture.is_none() {
                match self.create_capture(&dir) {
                    Ok(c) => self.capture = Some(c),
                    Err(err) => { eprintln!("capture setup failed: {err}"); self.dump_remaining = 0; }
                }
            }
            if self.dump_remaining > 0 {
                self.dump_remaining -= 1;
                capture_index = Some(self.dump_index);
                // One metadata row per dumped frame, so glitch frames can be explained.
                let now_q = pose.lock().unwrap().q;
                let delta = e.render_q.map(|rq| 2.0 * (rq[0] * now_q[0] + rq[1] * now_q[1] + rq[2] * now_q[2] + rq[3] * now_q[3]).abs().min(1.0).acos().to_degrees());
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("meta.csv")) {
                    if self.dump_index == 0 { let _ = writeln!(f, "dump_index,steamvr_frame,left_set,left_slot,age_ms,render_vs_now_deg,fence_supported,fence_pending"); }
                    let _ = writeln!(f, "{},{},{},{},{:.1},{},{},{}", self.dump_index, e.frame, e.slot.0, e.slot.1, e.age_ms, delta.map(|d| format!("{d:.2}")).unwrap_or_default(), e.fence_supported as u8, e.fence_pending as u8);
                }
                self.dump_index += 1;
            }
        }
        // Tell the driver, which paces SteamVR's vsync on it (type 5: [5, sequence, ns_lo, ns_hi]). Without present wait, ns is 0:
        // acquire returning when the display took the previous frame is then the best estimate, taken on arrival.
        let fd = DRIVER_FD.load(Ordering::Relaxed);
        if fd >= 0 {
            self.vsync_seq = self.vsync_seq.wrapping_add(1);
            let mut w = [0u32; 16];
            w[0] = 5;
            w[1] = self.vsync_seq;
            w[2] = vblank_ns as u32;
            w[3] = (vblank_ns >> 32) as u32;
            libc::send(fd, w.as_ptr() as *const _, 64, libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT);
        }
        let cmd = self.cmd;
        self.device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        self.device.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
        let image = self.images[idx as usize];
        let mut wait = vec![self.image_available];
        let mut stages = vec![vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        match eyes {
            Some(e) => {
                for s in self.import_wait_fds(e.wait_fds) {
                    wait.push(s);
                    stages.push(vk::PipelineStageFlags::TRANSFER | vk::PipelineStageFlags::FRAGMENT_SHADER);
                }
                // Reproject when we have the pose SteamVR rendered for and a current tracked pose; otherwise plain blit.
                let mut delta = None;
                let rows = if self.warp.is_some() {
                    e.render_q.and_then(|rq| {
                        let p = *pose.lock().unwrap();
                        if p.valid {
                            let d = (rq[0] * p.q[0] + rq[1] * p.q[1] + rq[2] * p.q[2] + rq[3] * p.q[3]).abs().min(1.0);
                            delta = Some(2.0 * d.acos().to_degrees());
                            Some(warp::push_rows(&warp::view_delta(rq, p.q)))
                        } else { None }
                    })
                } else {
                    None
                };
                if let Some(d) = delta { self.last_delta_deg = d; }
                match rows {
                    Some(r) => self.record_warp(cmd, idx as usize, image, &e, r),
                    None => self.record_blit(cmd, image, e.imgs, e.bounds),
                }
            }
            None => self.record_pattern(cmd, idx as usize, image, t),
        }
        if let Some(i) = capture_index {
            self.record_capture(cmd, image);
            self.capture_pending = Some(i);
        }
        self.device.end_command_buffer(cmd)?;

        let signal = [self.render_done[idx as usize]];
        let cmds = [cmd];
        self.device.queue_submit(self.queue,
            &[vk::SubmitInfo::default().wait_semaphores(&wait).wait_dst_stage_mask(&stages).command_buffers(&cmds).signal_semaphores(&signal)],
            self.in_flight)?;
        let swapchains = [self.swapchain];
        let indices = [idx];
        let ids = [self.present_id + 1];
        let mut present_id = vk::PresentIdKHR::default().present_ids(&ids);
        let mut info = vk::PresentInfoKHR::default().wait_semaphores(&signal).swapchains(&swapchains).image_indices(&indices);
        if self.present_wait.is_some() {
            info = info.push_next(&mut present_id);
            self.present_id += 1;
        }
        match self.swapchain_loader.queue_present(self.queue, &info) {
            Ok(_) | Err(vk::Result::SUBOPTIMAL_KHR) => {}
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.create_swapchain(window.inner_size())?,
            Err(e) => return Err(e.into()),
        }
        let _ = self.queue_family;
        Ok(())
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for s in self.render_done.drain(..) { self.device.destroy_semaphore(s, None); }
            for v in self.views.drain(..) { self.device.destroy_image_view(v, None); }
            self.device.destroy_fence(self.in_flight, None);
            self.device.destroy_semaphore(self.image_available, None);
            for s in self.read_ready { self.device.destroy_semaphore(s, None); }
            if let Some(c) = self.capture.take() {
                self.device.destroy_buffer(c.buf, None);
                self.device.free_memory(c.mem, None);
            }
            if let Some(w) = self.warp.take() {
                self.device.destroy_pipeline(w.pipeline, None);
                self.device.destroy_pipeline_layout(w.layout, None);
                self.device.destroy_descriptor_pool(w.pool, None);
                self.device.destroy_descriptor_set_layout(w.desc_layout, None);
                self.device.destroy_sampler(w.sampler, None);
            }
            self.device.destroy_command_pool(self.pool, None);
            self.swapchain_loader.destroy_swapchain(self.swapchain, None);
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}

struct App {
    monitor_name: String,
    window: Option<Window>,
    gfx: Option<Gfx>,
    start: Instant,
    frames: u32,
    last_report: Instant,
    shared: Arc<Mutex<Shared>>,
    last_monitor_check: Instant,
    reproject: bool,
    dump_dir: Option<std::path::PathBuf>,
    dump_count: u32,
    pose: Arc<Mutex<tracking::PoseState>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        println!("monitors:");
        let mut chosen = None;
        for m in el.available_monitors() {
            let name = m.name().unwrap_or_default();
            let s = m.size();
            println!("  {name}: {}x{} at {:?}, refresh {:?} mHz", s.width, s.height, m.position(), m.refresh_rate_millihertz());
            if name == self.monitor_name {
                chosen = Some(m);
            }
        }
        if chosen.is_none() {
            eprintln!("monitor '{}' not found, using the compositor's choice", self.monitor_name);
        }
        let window = el
            .create_window(Window::default_attributes().with_title("XREAL presenter").with_fullscreen(Some(Fullscreen::Borderless(chosen))))
            .expect("create window");
        let mut gfx = unsafe { Gfx::new(&window, self.reproject) }.expect("vulkan init");
        gfx.dump_dir = self.dump_dir.clone();
        gfx.dump_count = self.dump_count;
        self.gfx = Some(gfx);
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() && event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) => el.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = &mut self.gfx {
                    let _ = unsafe { g.create_swapchain(size) };
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(g), Some(w)) = (&mut self.gfx, &self.window) {
                    if let Err(e) = unsafe { g.draw(self.start.elapsed().as_secs_f32(), w, &self.shared, &self.pose) } {
                        eprintln!("draw error: {e}");
                        el.exit();
                    }
                    self.frames += 1;
                    if self.last_report.elapsed().as_secs_f32() >= 5.0 {
                        let secs = self.last_report.elapsed().as_secs_f32();
                        let mean_age = if g.new_frames > 0 { g.new_frame_age_ms.0 / g.new_frames as f32 } else { 0.0 };
                        let newest = format!("{:.1} new SteamVR frames/s (age mean {:.1} max {:.1} ms)", g.new_frames as f32 / secs, mean_age, g.new_frame_age_ms.1);
                        (g.new_frames, g.new_frame_age_ms) = (0, (0.0, 0.0));
                        // Judder check: during smooth head motion the steps should be even; a repeat (near 0) then a double step is judder.
                        let mut steps = std::mem::take(&mut g.render_steps_deg);
                        let newest = if steps.len() > 10 {
                            steps.sort_by(|a, b| a.total_cmp(b));
                            let median = steps[steps.len() / 2];
                            let repeats = steps.iter().filter(|&&d| d < 0.25 * median).count();
                            let doubles = steps.iter().filter(|&&d| d > 1.75 * median).count();
                            format!("{newest}, render pose step median {median:.3} deg, {repeats} near-repeats, {doubles} double steps")
                        } else { newest };
                        if g.reproject {
                            println!("{:.1} fps, {newest}, reprojection delta {:.2} deg, fallback frames {}", self.frames as f32 / secs, g.last_delta_deg, g.fallbacks);
                        } else {
                            println!("{:.1} fps, {newest}, fallback frames {}", self.frames as f32 / secs, g.fallbacks);
                        }
                        self.frames = 0;
                        self.last_report = Instant::now();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            // When the glasses re-plug (mode change, sleep and wake) the compositor can drop our window onto another
            // output. Keep putting it back on the glasses' output whenever that output exists.
            if self.last_monitor_check.elapsed() >= std::time::Duration::from_millis(500) {
                self.last_monitor_check = Instant::now();
                let current = w.current_monitor().and_then(|m| m.name());
                if current.as_deref() != Some(self.monitor_name.as_str()) {
                    if let Some(m) = w.available_monitors().find(|m| m.name().as_deref() == Some(self.monitor_name.as_str())) {
                        println!("window is on {:?}; moving it to {}", current, self.monitor_name);
                        w.set_fullscreen(Some(Fullscreen::Borderless(Some(m))));
                    }
                }
            }
            w.request_redraw();
        }
    }
}

fn main() {
    let mut monitor_name = "DP-1".to_string();
    let mut reproject = false;
    let mut sim_pose = false;
    let mut sim_yaw = 25.0f64;
    let mut sim_pitch = 0.0f64;
    let mut sim_pitch_amp = 12.0f64;
    let mut dump_count = 30u32;
    let mut dump_dir: Option<std::path::PathBuf> = None;
    if let Some(d) = &dump_dir { let _ = d; }
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--monitor" {
            monitor_name = args.next().expect("--monitor needs a name");
        } else if a == "--reproject" {
            reproject = true;
        } else if a == "--sim-pose" {
            sim_pose = true;
        } else if a == "--sim-pitch" {
            sim_pitch = args.next().and_then(|v| v.parse().ok()).expect("--sim-pitch needs degrees");
        } else if a == "--sim-pitch-amp" {
            sim_pitch_amp = args.next().and_then(|v| v.parse().ok()).expect("--sim-pitch-amp needs degrees");
        } else if a == "--sim-yaw" {
            sim_yaw = args.next().and_then(|v| v.parse().ok()).expect("--sim-yaw needs degrees");
        } else if a == "--dump-frames" {
            dump_count = args.next().and_then(|v| v.parse().ok()).expect("--dump-frames needs a number");
        } else if a == "--dump" {
            dump_dir = Some(std::path::PathBuf::from(args.next().expect("--dump needs a directory")));
        }
    }
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Poll);
    let shared = Arc::new(Mutex::new(Shared::default()));
    let pose = Arc::new(Mutex::new(tracking::PoseState::default()));
    let pose_for_app = pose.clone();
    if sim_pose {
        println!("SIMULATED head sweep instead of the IMU");
        let p = pose.clone();
        std::thread::spawn(move || tracking::run_sim(p, sim_yaw, sim_pitch, sim_pitch_amp));
    } else {
        let p = pose.clone();
        std::thread::spawn(move || tracking::run(p));
    }
    { let (sh, p) = (shared.clone(), pose.clone()); std::thread::spawn(move || link_thread(sh, p)); }
    let mut app = App { monitor_name, window: None, gfx: None, start: Instant::now(), frames: 0, last_report: Instant::now(), shared, last_monitor_check: Instant::now(), reproject, dump_dir, dump_count, pose: pose_for_app };
    el.run_app(&mut app).expect("run");
}
