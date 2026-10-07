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
use std::ffi::CStr;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

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
        let dev_exts = [khr::swapchain::NAME.as_ptr()];
        let mut f13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let device = instance.create_device(
            phys,
            &vk::DeviceCreateInfo::default().queue_create_infos(&qci).enabled_extension_names(&dev_exts).push_next(&mut f13),
            None,
        )?;
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
            .find(|f| f.format == vk::Format::B8G8R8A8_UNORM)
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
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
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

    /// Draw one frame of the stereo test pattern. `t` is seconds since start.
    unsafe fn draw(&mut self, t: f32, window: &Window) -> Result<(), Box<dyn std::error::Error>> {
        self.device.wait_for_fences(&[self.in_flight], true, u64::MAX)?;
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
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_attachment = vk::ImageMemoryBarrier::default()
            .image(image).subresource_range(range)
            .old_layout(vk::ImageLayout::UNDEFINED).new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);
        self.device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(), &[], &[], &[to_attachment]);

        let (w, h) = (self.extent.width as i32, self.extent.height as i32);
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(self.views[idx as usize]).image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
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
                    if let Err(e) = unsafe { g.draw(self.start.elapsed().as_secs_f32(), w) } {
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
    let mut app = App { monitor_name, window: None, gfx: None, start: Instant::now(), frames: 0, last_report: Instant::now() };
    el.run_app(&mut app).expect("run");
}
