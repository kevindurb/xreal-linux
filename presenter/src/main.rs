//! Fullscreen Vulkan presenter for XREAL glasses.
//!
//! This first version draws a side-by-side stereo *test pattern* on the glasses' output so we can check that the
//! glasses really show a different image to each eye in their SBS mode. It will grow into the thing that imports
//! SteamVR's per-eye textures and presents them.
//!
//! usage: xreal-presenter [--monitor NAME] [--reproject] [--guard-ms N] [--wait-fences] [--sim-pose [--sim-yaw DEG] [--sim-pitch DEG] [--sim-pitch-amp DEG]] [--dump DIR [--dump-frames N]]      (default monitor name: DP-1)
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

/// Ask the kernel whether the GPU work that writes this dma-buf has finished (DMA_BUF_IOCTL_EXPORT_SYNC_FILE).
/// Returns (supported, still_pending_after_waiting, milliseconds_waited). `timeout_ms` of 0 only checks.
fn fence_state(dmabuf_fd: RawFd, timeout_ms: i32) -> (bool, bool, f32) {
    #[repr(C)]
    struct ExportSyncFile { flags: u32, fd: i32 }
    const DMA_BUF_IOCTL_EXPORT_SYNC_FILE: libc::c_ulong = 0xC008_6202; // _IOWR('b', 2, struct dma_buf_export_sync_file)
    let mut req = ExportSyncFile { flags: 1, fd: -1 }; // DMA_BUF_SYNC_READ: wait for the writers
    unsafe {
        if libc::ioctl(dmabuf_fd, DMA_BUF_IOCTL_EXPORT_SYNC_FILE, &mut req) != 0 || req.fd < 0 {
            return (false, false, 0.0);
        }
        let t = Instant::now();
        let mut p = libc::pollfd { fd: req.fd, events: libc::POLLIN, revents: 0 };
        let r = libc::poll(&mut p, 1, timeout_ms);
        libc::close(req.fd);
        (true, r == 0, t.elapsed().as_secs_f32() * 1000.0)
    }
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
    previous: Option<PresentMsg>, // the frame before `present`, which is certain to be complete
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
            // Stream the head pose to the driver (type 4: [4, ts_lo, ts_hi, w, x, y, z as f32 bits, valid, wx, wy, wz as f32 bits]).
            let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
            {
                let (alive, pose) = (alive.clone(), pose.clone());
                std::thread::spawn(move || {
                    while alive.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        let p = *pose.lock().unwrap();
                        let w: [u32; 16] = [4, p.timestamp_ns as u32, (p.timestamp_ns >> 32) as u32, p.q[0].to_bits(), p.q[1].to_bits(),
                                            p.q[2].to_bits(), p.q[3].to_bits(), p.valid as u32, p.omega[0].to_bits(), p.omega[1].to_bits(), p.omega[2].to_bits(), 0, 0, 0, 0, 0];
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
                        sh.previous = sh.present;
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
            sh.previous = None;
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

/// The two eye images to show, with what reprojection needs.
struct Eyes {
    imgs: [(vk::Image, u32, u32); 2],
    sets: [vk::DescriptorSet; 2],
    render_q: Option<[f32; 4]>,
    bounds: [[f32; 4]; 2],
    frame: u32,            // SteamVR's present counter for this frame
    slot: (u32, u32),      // left eye (set id, index)
    used_previous: bool,   // the frame-age guard picked the previous frame
    age_ms: f32,           // how old the chosen frame was
    fence_supported: bool, // the kernel could export the writer's fence
    fence_pending: bool,   // the writer had not finished when we looked (after any wait)
    fence_ms: f32,         // how long we waited for it
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
    wait_fences: bool,
    pub fallbacks: u32,  // frames drawn as the test pattern while the driver was connected (should stay 0)
    guard_ms: u64,       // how old a presented frame must be before we show it
    last_delta_deg: f32, // head rotation between SteamVR's render pose and now, last warped frame
}

impl Gfx {
    unsafe fn new(window: &Window, reproject: bool, guard_ms: u64) -> Result<Gfx, Box<dyn std::error::Error>> {
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
        let dev_exts = [khr::swapchain::NAME.as_ptr(), khr::external_memory_fd::NAME.as_ptr()];
        let mut f13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let device = instance.create_device(
            phys,
            &vk::DeviceCreateInfo::default().queue_create_infos(&qci).enabled_extension_names(&dev_exts).push_next(&mut f13),
            None,
        )?;
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

        let mut g = Gfx {
            _entry: entry, instance, surface_loader, surface, phys, device, queue, queue_family, swapchain_loader,
            swapchain: vk::SwapchainKHR::null(), images: vec![], views: vec![], format: vk::Format::B8G8R8A8_UNORM,
            extent: vk::Extent2D { width: 1, height: 1 }, pool, cmd, image_available, render_done: vec![], in_flight,
            mem_props, seen: Default::default(), vsync_seq: 0, warp: None, reproject, dump_dir: None, dump_remaining: 0, dump_count: 30, dump_index: 0, wait_fences: false, fallbacks: 0, guard_ms, last_delta_deg: 0.0,
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
        self.swapchain = self.swapchain_loader.create_swapchain(
            &vk::SwapchainCreateInfoKHR::default()
                .surface(self.surface)
                .min_image_count(count)
                .image_format(fmt.format)
                .image_color_space(fmt.color_space)
                .image_extent(self.extent)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
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

    /// Debug: read one eye image back to the CPU and write it as raw RGBA (sRGB bytes) into `dir`.
    unsafe fn dump_eye(&self, dir: &std::path::Path, eye: (vk::Image, u32, u32), index: u32, tag: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (img, full_w, full_h) = eye;
        // Only the middle of the picture (where the gaze pointer is): keeps long captures small.
        let (w, h) = (full_w.min(640), full_h.min(400));
        let (x0, y0) = ((full_w - w) / 2, (full_h - h) / 2);
        let size = (w as u64) * (h as u64) * 4;
        let buf = self.device.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(vk::BufferUsageFlags::TRANSFER_DST), None)?;
        let reqs = self.device.get_buffer_memory_requirements(buf);
        let want = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let mt = (0..self.mem_props.memory_type_count)
            .find(|&j| reqs.memory_type_bits & (1 << j) != 0 && self.mem_props.memory_types[j as usize].property_flags.contains(want))
            .ok_or("no host-visible memory type")?;
        let mem = self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(reqs.size).memory_type_index(mt), None)?;
        self.device.bind_buffer_memory(buf, mem, 0)?;
        let cb = self.device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default().command_pool(self.pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1),
        )?[0];
        self.device.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let back = if self.warp.is_some() { vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL } else { vk::ImageLayout::TRANSFER_SRC_OPTIMAL };
        let to_src = vk::ImageMemoryBarrier::default().image(img).subresource_range(range)
            .old_layout(vk::ImageLayout::UNDEFINED).new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE).dst_access_mask(vk::AccessFlags::TRANSFER_READ);
        self.device.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[to_src]);
        let region = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
            .image_offset(vk::Offset3D { x: x0 as i32, y: y0 as i32, z: 0 })
            .image_extent(vk::Extent3D { width: w, height: h, depth: 1 });
        self.device.cmd_copy_image_to_buffer(cb, img, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, buf, &[region]);
        let restore = vk::ImageMemoryBarrier::default().image(img).subresource_range(range)
            .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL).new_layout(back)
            .src_access_mask(vk::AccessFlags::TRANSFER_READ).dst_access_mask(vk::AccessFlags::SHADER_READ);
        self.device.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::ALL_COMMANDS, vk::DependencyFlags::empty(), &[], &[], &[restore]);
        self.device.end_command_buffer(cb)?;
        let fence = self.device.create_fence(&vk::FenceCreateInfo::default(), None)?;
        let cbs = [cb];
        self.device.queue_submit(self.queue, &[vk::SubmitInfo::default().command_buffers(&cbs)], fence)?;
        self.device.wait_for_fences(&[fence], true, u64::MAX)?;
        let ptr = self.device.map_memory(mem, 0, size, vk::MemoryMapFlags::empty())? as *const u8;
        let bytes = std::slice::from_raw_parts(ptr, size as usize);
        std::fs::write(dir.join(format!("frame_{index:04}_{tag}_{w}x{h}.rgba")), bytes)?;
        self.device.unmap_memory(mem);
        self.device.destroy_fence(fence, None);
        self.device.free_command_buffers(self.pool, &cbs);
        self.device.destroy_buffer(buf, None);
        self.device.free_memory(mem, None);
        Ok(())
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
        // SteamVR calls Present when it has *submitted* the GPU work, not when it has finished, and nothing here waits on its
        // fences. Use a new frame only once it is a few ms old and otherwise show the previous, certainly complete one.
        let latest = sh.present?;
        let use_latest = latest.at.elapsed() >= std::time::Duration::from_millis(self.guard_ms);
        let p = if use_latest { latest } else { sh.previous.unwrap_or(latest) };
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
        // Is the GPU work that wrote these two images finished? With --wait-fences, wait for it (up to 25 ms).
        let (mut fence_supported, mut fence_pending, mut fence_ms) = (false, false, 0.0f32);
        if self.wait_fences || self.dump_dir.is_some() {
            for (sid, idx) in [p.left, p.right] {
                if let Some(set) = sh.sets.get(&sid) {
                    let (sup, pend, ms) = fence_state(set.sync_fds[idx as usize], if self.wait_fences { 25 } else { 0 });
                    fence_supported |= sup;
                    fence_pending |= pend;
                    fence_ms += ms;
                }
            }
        }
        Some(Eyes { imgs: out, sets, render_q: p.render_q, bounds: p.bounds, frame: p.frame, slot: p.left, used_previous: !use_latest, age_ms: p.at.elapsed().as_secs_f32() * 1000.0, fence_supported, fence_pending, fence_ms })
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
        let eyes = self.prepare_eyes(shared);
        if eyes.is_none() && shared.lock().unwrap().connected {
            self.fallbacks += 1;
        }
        if let (Some(dir), Some(e)) = (self.dump_dir.clone(), &eyes) {
            // Debug: `touch DIR/trigger` dumps the next frames (both eyes, centre crop) as raw RGBA.
            if self.dump_remaining == 0 && dir.join("trigger").exists() {
                let _ = std::fs::remove_file(dir.join("trigger"));
                self.dump_remaining = self.dump_count;
                println!("dumping {} frames to {}", self.dump_count, dir.display());
            }
            if self.dump_remaining > 0 {
                self.dump_remaining -= 1;
                for (k, tag) in ["L", "R"].into_iter().enumerate() {
                    if let Err(err) = self.dump_eye(&dir, e.imgs[k], self.dump_index, tag) {
                        eprintln!("dump failed: {err}");
                        self.dump_remaining = 0;
                        break;
                    }
                }
                // Read the same images again after a pause: if a frame changes between the two reads, SteamVR was still drawing it.
                std::thread::sleep(std::time::Duration::from_millis(60));
                for (k, tag) in ["L2", "R2"].into_iter().enumerate() {
                    if let Err(err) = self.dump_eye(&dir, e.imgs[k], self.dump_index, tag) {
                        eprintln!("dump failed: {err}");
                        self.dump_remaining = 0;
                        break;
                    }
                }
                // One metadata row per dumped frame, so glitch frames can be explained.
                let now_q = pose.lock().unwrap().q;
                let delta = e.render_q.map(|rq| 2.0 * (rq[0] * now_q[0] + rq[1] * now_q[1] + rq[2] * now_q[2] + rq[3] * now_q[3]).abs().min(1.0).acos().to_degrees());
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("meta.csv")) {
                    if self.dump_index == 0 { let _ = writeln!(f, "dump_index,steamvr_frame,left_set,left_slot,used_previous,age_ms,render_vs_now_deg,fence_supported,fence_pending,fence_ms"); }
                    let _ = writeln!(f, "{},{},{},{},{},{:.1},{},{},{},{:.2}", self.dump_index, e.frame, e.slot.0, e.slot.1, e.used_previous as u8, e.age_ms, delta.map(|d| format!("{d:.2}")).unwrap_or_default(), e.fence_supported as u8, e.fence_pending as u8, e.fence_ms);
                }
                self.dump_index += 1;
            }
        }
        let (idx, _) = match self.swapchain_loader.acquire_next_image(self.swapchain, u64::MAX, self.image_available, vk::Fence::null()) {
            Ok(r) => r,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return self.create_swapchain(window.inner_size()),
            Err(e) => return Err(e.into()),
        };
        self.device.reset_fences(&[self.in_flight])?;
        // acquire returns when the display has taken the previous frame, so this is our best estimate of a vblank: tell the
        // driver, which turns it into SteamVR's vsync event (type 5: [5, sequence]).
        let fd = DRIVER_FD.load(Ordering::Relaxed);
        if fd >= 0 {
            self.vsync_seq = self.vsync_seq.wrapping_add(1);
            let mut w = [0u32; 16];
            w[0] = 5;
            w[1] = self.vsync_seq;
            libc::send(fd, w.as_ptr() as *const _, 64, libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT);
        }
        let cmd = self.cmd;
        self.device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        self.device.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
        let image = self.images[idx as usize];
        match eyes {
            Some(e) => {
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
        self.device.end_command_buffer(cmd)?;

        let wait = [self.image_available];
        let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let signal = [self.render_done[idx as usize]];
        let cmds = [cmd];
        self.device.queue_submit(self.queue,
            &[vk::SubmitInfo::default().wait_semaphores(&wait).wait_dst_stage_mask(&stages).command_buffers(&cmds).signal_semaphores(&signal)],
            self.in_flight)?;
        let swapchains = [self.swapchain];
        let indices = [idx];
        match self.swapchain_loader.queue_present(self.queue,
            &vk::PresentInfoKHR::default().wait_semaphores(&signal).swapchains(&swapchains).image_indices(&indices)) {
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
    guard_ms: u64,
    dump_dir: Option<std::path::PathBuf>,
    dump_count: u32,
    wait_fences: bool,
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
        let mut gfx = unsafe { Gfx::new(&window, self.reproject, self.guard_ms) }.expect("vulkan init");
        gfx.dump_dir = self.dump_dir.clone();
        gfx.dump_count = self.dump_count;
        gfx.wait_fences = self.wait_fences;
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
                        if g.reproject {
                            println!("{:.1} fps, reprojection delta {:.2} deg, fallback frames {}", self.frames as f32 / self.last_report.elapsed().as_secs_f32(), g.last_delta_deg, g.fallbacks);
                        } else {
                            println!("{:.1} fps, fallback frames {}", self.frames as f32 / self.last_report.elapsed().as_secs_f32(), g.fallbacks);
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
    let mut guard_ms = 4u64;
    let mut sim_pose = false;
    let mut sim_yaw = 25.0f64;
    let mut sim_pitch = 0.0f64;
    let mut sim_pitch_amp = 12.0f64;
    let mut wait_fences = false;
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
        } else if a == "--wait-fences" {
            wait_fences = true;
        } else if a == "--sim-yaw" {
            sim_yaw = args.next().and_then(|v| v.parse().ok()).expect("--sim-yaw needs degrees");
        } else if a == "--dump-frames" {
            dump_count = args.next().and_then(|v| v.parse().ok()).expect("--dump-frames needs a number");
        } else if a == "--dump" {
            dump_dir = Some(std::path::PathBuf::from(args.next().expect("--dump needs a directory")));
        } else if a == "--guard-ms" {
            guard_ms = args.next().and_then(|v| v.parse().ok()).expect("--guard-ms needs a number");
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
    let mut app = App { monitor_name, window: None, gfx: None, start: Instant::now(), frames: 0, last_report: Instant::now(), shared, last_monitor_check: Instant::now(), reproject, guard_ms, dump_dir, dump_count, wait_fences, pose: pose_for_app };
    el.run_app(&mut app).expect("run");
}
