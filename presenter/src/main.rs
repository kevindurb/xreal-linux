//! Fullscreen Vulkan presenter for XREAL glasses.
//!
//! This first version draws a side-by-side stereo *test pattern* on the glasses' output so we can check that the
//! glasses really show a different image to each eye in their SBS mode. It will grow into the thing that imports
//! SteamVR's per-eye textures and presents them.
//!
//! usage: xreal-presenter [--monitor NAME]      (default monitor name: DP-1)
//!
//! Left half of the screen = left eye (red tint), right half = right eye (blue tint). A green square slides across
//! each half; its position differs by a few pixels between the eyes, so in a working stereo mode it appears to
//! float at a different depth from the frame. White borders and a white centre line show the exact edges.

use ash::{khr, vk, Device, Entry, Instance};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::fd::RawFd;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

// ---- link to the SteamVR driver -------------------------------------------------------------------------------
// The driver (driver/src/xreal_driver.cpp) connects to a SOCK_SEQPACKET unix socket and sends fixed 16-word messages:
//   1 SET     [1, set_id, width, height, vk_format, usage, create_flags] + 3 fds (SCM_RIGHTS)
//   2 DESTROY [2, set_id]
//   3 PRESENT [3, left_set, left_index, right_set, right_index, frame_number]

struct ImportedImage {
    image: vk::Image,
    memory: vk::DeviceMemory,
}

struct SetInfo {
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    flags: u32,
    fds: [RawFd; 3],
    images: Option<Vec<ImportedImage>>, // imported lazily, on first use
}

#[derive(Clone, Copy)]
struct PresentMsg {
    left: (u32, u32),
    right: (u32, u32),
    frame: u32,
}

#[derive(Default)]
struct Shared {
    sets: HashMap<u32, SetInfo>,
    destroyed: Vec<u32>,
    present: Option<PresentMsg>,
    connected: bool,
}

fn link_thread(shared: Arc<Mutex<Shared>>) {
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
            shared.lock().unwrap().connected = true;
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
                                      fds: [fds[0], fds[1], fds[2]], images: None },
                        );
                    }
                    2 => sh.destroyed.push(words[1]),
                    3 => sh.present = Some(PresentMsg { left: (words[1], words[2]), right: (words[3], words[4]), frame: words[5] }),
                    _ => {
                        for fd in fds {
                            libc::close(fd);
                        }
                    }
                }
            }
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
}

impl Gfx {
    unsafe fn new(window: &Window) -> Result<Gfx, Box<dyn std::error::Error>> {
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
            mem_props, seen: Default::default(),
        };
        g.create_swapchain(window.inner_size())?;
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
    unsafe fn record_blit(&mut self, cmd: vk::CommandBuffer, dst: vk::Image, eyes: [(vk::Image, u32, u32); 2]) {
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
            let blit = vk::ImageBlit::default()
                .src_subresource(layers).src_offsets([vk::Offset3D { x: 0, y: 0, z: 0 }, vk::Offset3D { x: sw as i32, y: sh as i32, z: 1 }])
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

    /// Free destroyed sets, import the sets the latest PRESENT refers to, and return the two eye images (if any).
    unsafe fn prepare_eyes(&mut self, shared: &Mutex<Shared>) -> Option<[(vk::Image, u32, u32); 2]> {
        let mut guard = shared.lock().unwrap();
        let sh = &mut *guard;
        for id in sh.destroyed.drain(..) {
            if let Some(set) = sh.sets.remove(&id) {
                match set.images {
                    Some(imgs) => {
                        for im in imgs {
                            self.seen.remove(&im.image);
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
        let p = sh.present?;
        let mut out = [(vk::Image::null(), 0u32, 0u32); 2];
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
        }
        Some(out)
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
            out.push(ImportedImage { image, memory });
        }
        Ok(out)
    }

    /// Draw one frame: the driver's eyes if it is presenting, otherwise the stereo test pattern.
    unsafe fn draw(&mut self, t: f32, window: &Window, shared: &Mutex<Shared>) -> Result<(), Box<dyn std::error::Error>> {
        self.device.wait_for_fences(&[self.in_flight], true, u64::MAX)?;
        let eyes = self.prepare_eyes(shared);
        let (idx, _) = match self.swapchain_loader.acquire_next_image(self.swapchain, u64::MAX, self.image_available, vk::Fence::null()) {
            Ok(r) => r,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return self.create_swapchain(window.inner_size()),
            Err(e) => return Err(e.into()),
        };
        self.device.reset_fences(&[self.in_flight])?;
        let cmd = self.cmd;
        self.device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        self.device.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
        let image = self.images[idx as usize];
        match eyes {
            Some(e) => self.record_blit(cmd, image, e),
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
        self.gfx = Some(unsafe { Gfx::new(&window) }.expect("vulkan init"));
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
                    if let Err(e) = unsafe { g.draw(self.start.elapsed().as_secs_f32(), w, &self.shared) } {
                        eprintln!("draw error: {e}");
                        el.exit();
                    }
                    self.frames += 1;
                    if self.last_report.elapsed().as_secs_f32() >= 5.0 {
                        println!("{:.1} fps", self.frames as f32 / self.last_report.elapsed().as_secs_f32());
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
            w.request_redraw();
        }
    }
}

fn main() {
    let mut monitor_name = "DP-1".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--monitor" {
            monitor_name = args.next().expect("--monitor needs a name");
        }
    }
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Poll);
    let shared = Arc::new(Mutex::new(Shared::default()));
    { let sh = shared.clone(); std::thread::spawn(move || link_thread(sh)); }
    let mut app = App { monitor_name, window: None, gfx: None, start: Instant::now(), frames: 0, last_report: Instant::now(), shared };
    el.run_app(&mut app).expect("run");
}
