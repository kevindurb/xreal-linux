//! A userspace input device over `/dev/uinput`: a virtual gamepad or mouse that other applications see as an
//! ordinary device. No root and no udev rule are needed where the active session user has access to the node
//! (the `uaccess` ACL on Bazzite does this). Uses raw `libc` ioctls so no new crate is added.
//!
//! See `openspec/changes/add-headlook-mode` (design decision 3).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, RawFd};

/// Device names, documented in `presenter/README.md`.
pub const GAMEPAD_NAME: &str = "XREAL Head-look Gamepad";
pub const MOUSE_NAME: &str = "XREAL Head-look Mouse";

// Event types.
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
// Relative axes.
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
// Absolute axes (public so the head-look mapping can name the axis it drives).
pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
// Buttons (present so the device looks like a gamepad; unused for head-look).
const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
// Mouse buttons. Head-look never presses them, but udev's input-id classifies a relative device as a mouse
// (ID_INPUT_MOUSE) only when it has buttons, and without that classification the compositor does not take it.
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
// Bus and product ids.
const BUS_USB: u16 = 0x03;
const PRODUCT_GAMEPAD: u16 = 0x0a01;
const PRODUCT_MOUSE: u16 = 0x0a02;

// The joystick axis range (the conventional -32767..32767).
const ABS_MIN: i32 = -32767;
const ABS_MAX: i32 = 32767;

// ioctl encodings: _IO / _IOW from <linux/uinput.h> and <asm-generic/ioctl.h>.
const IOC_WRITE: u32 = 1;
const fn ioc(dir: u32, type_: u32, nr: u32, size: u32) -> u64 {
    ((dir << 30) | (size << 16) | (type_ << 8) | nr) as u64
}
const UI_DEV_CREATE: u64 = ioc(0, b'U' as u32, 1, 0);
const UI_DEV_DESTROY: u64 = ioc(0, b'U' as u32, 2, 0);
const UI_DEV_SETUP: u64 = ioc(IOC_WRITE, b'U' as u32, 3, std::mem::size_of::<UinputSetup>() as u32);
const UI_SET_EVBIT: u64 = ioc(IOC_WRITE, b'U' as u32, 100, 4);
const UI_SET_KEYBIT: u64 = ioc(IOC_WRITE, b'U' as u32, 101, 4);
const UI_SET_RELBIT: u64 = ioc(IOC_WRITE, b'U' as u32, 102, 4);
const UI_SET_ABSBIT: u64 = ioc(IOC_WRITE, b'U' as u32, 103, 4);
const UI_ABS_SETUP: u64 = ioc(IOC_WRITE, b'U' as u32, 4, std::mem::size_of::<UinputAbsSetup>() as u32);

#[repr(C)]
#[derive(Clone, Copy)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InputAbsinfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UinputAbsSetup {
    code: u16,
    pad: u16,
    absinfo: InputAbsinfo,
}

/// A kernel `struct input_event` (64-bit layout).
#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    time: libc::timeval,
    type_: u16,
    code: u16,
    value: i32,
}

fn encode(ev: &InputEvent) -> [u8; std::mem::size_of::<InputEvent>()] {
    let mut out = [0u8; std::mem::size_of::<InputEvent>()];
    unsafe {
        std::ptr::copy_nonoverlapping(ev as *const _ as *const u8, out.as_mut_ptr(), out.len());
    }
    out
}

/// Which kind of device a [`UinputDevice`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Gamepad,
    Mouse,
}

/// An open virtual input device. Removed from the system when dropped.
pub struct UinputDevice {
    file: File,
    kind: DeviceKind,
}

impl UinputDevice {
    pub fn kind(&self) -> DeviceKind {
        self.kind
    }

    /// Create a virtual gamepad with two stick axes (x right, y up) and two buttons.
    pub fn create_gamepad(name: &str) -> io::Result<Self> {
        let file = Self::open()?;
        let fd = file.as_raw_fd();
        unsafe {
            set_bit(fd, UI_SET_EVBIT, EV_SYN)?;
            set_bit(fd, UI_SET_EVBIT, EV_ABS)?;
            set_bit(fd, UI_SET_EVBIT, EV_KEY)?;
            set_bit(fd, UI_SET_ABSBIT, ABS_X)?;
            set_bit(fd, UI_SET_ABSBIT, ABS_Y)?;
            set_bit(fd, UI_SET_KEYBIT, BTN_SOUTH)?;
            set_bit(fd, UI_SET_KEYBIT, BTN_EAST)?;
            for code in [ABS_X, ABS_Y] {
                set_abs(fd, code, ABS_MIN, ABS_MAX)?;
            }
            create(fd, name, PRODUCT_GAMEPAD)?;
        }
        Ok(UinputDevice { file, kind: DeviceKind::Gamepad })
    }

    /// Create a virtual relative-pointer (mouse). The three buttons are declared but never pressed: they are what
    /// make udev/libinput classify the device as a mouse rather than generic input.
    pub fn create_mouse(name: &str) -> io::Result<Self> {
        let file = Self::open()?;
        let fd = file.as_raw_fd();
        unsafe {
            set_bit(fd, UI_SET_EVBIT, EV_SYN)?;
            set_bit(fd, UI_SET_EVBIT, EV_REL)?;
            set_bit(fd, UI_SET_EVBIT, EV_KEY)?;
            set_bit(fd, UI_SET_RELBIT, REL_X)?;
            set_bit(fd, UI_SET_RELBIT, REL_Y)?;
            set_bit(fd, UI_SET_KEYBIT, BTN_LEFT)?;
            set_bit(fd, UI_SET_KEYBIT, BTN_RIGHT)?;
            set_bit(fd, UI_SET_KEYBIT, BTN_MIDDLE)?;
            create(fd, name, PRODUCT_MOUSE)?;
        }
        Ok(UinputDevice { file, kind: DeviceKind::Mouse })
    }

    fn open() -> io::Result<File> {
        OpenOptions::new().write(true).open("/dev/uinput")
    }

    /// Emit a gamepad absolute axis. `code` is the `ABS_*` code (e.g. [`ABS_X`]); `value` is -32767..32767.
    pub fn emit_abs(&mut self, code: u16, value: i32) -> io::Result<()> {
        self.write_event(EV_ABS, code, value)
    }

    /// Emit a relative pointer delta.
    pub fn emit_rel(&mut self, axis: u16, value: i32) -> io::Result<()> {
        let code = if axis == 0 { REL_X } else { REL_Y };
        self.write_event(EV_REL, code, value)
    }

    /// Emit the SYN_REPORT that ends a frame of events.
    pub fn syn(&mut self) -> io::Result<()> {
        self.write_event(EV_SYN, 0, 0)
    }

    fn write_event(&mut self, type_: u16, code: u16, value: i32) -> io::Result<()> {
        let ev = InputEvent { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, type_, code, value };
        self.file.write_all(&encode(&ev))
    }
}

impl Drop for UinputDevice {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY);
        }
    }
}

unsafe fn create(fd: RawFd, name: &str, product: u16) -> io::Result<()> {
    let mut setup: UinputSetup = std::mem::zeroed();
    setup.id = InputId { bustype: BUS_USB, vendor: 0x1d6b, product, version: 1 };
    let bytes = name.as_bytes();
    let n = bytes.len().min(setup.name.len() - 1);
    setup.name[..n].copy_from_slice(&bytes[..n]);
    if libc::ioctl(fd, UI_DEV_SETUP, &setup) < 0 {
        return Err(io::Error::last_os_error());
    }
    if libc::ioctl(fd, UI_DEV_CREATE) < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

unsafe fn set_bit(fd: RawFd, request: u64, bit: u16) -> io::Result<()> {
    if libc::ioctl(fd, request, bit as libc::c_int) < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

unsafe fn set_abs(fd: RawFd, code: u16, min: i32, max: i32) -> io::Result<()> {
    let setup = UinputAbsSetup {
        code,
        pad: 0,
        absinfo: InputAbsinfo { value: 0, minimum: min, maximum: max, fuzz: 16, flat: 128, resolution: 0 },
    };
    if libc::ioctl(fd, UI_ABS_SETUP, &setup) < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Convert a normalised axis (-1..1, y up) to the device's -32767..32767 (y down for the kernel).
pub fn gamepad_value(norm: f64) -> i32 {
    (norm.clamp(-1.0, 1.0) * 32767.0).round() as i32
}

/// Convert a pointer delta (counts) to the kernel's i32.
pub fn mouse_value(counts: f64) -> i32 {
    counts.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_encodings_match_linux_uinput_h() {
        // Values from <linux/uinput.h> on x86_64.
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_DESTROY, 0x5502);
        assert_eq!(UI_DEV_SETUP, 0x405c5503);
        assert_eq!(UI_ABS_SETUP, 0x401c5504);
        assert_eq!(UI_SET_EVBIT, 0x40045564);
        assert_eq!(UI_SET_KEYBIT, 0x40045565);
        assert_eq!(UI_SET_RELBIT, 0x40045566);
        assert_eq!(UI_SET_ABSBIT, 0x40045567);
    }

    #[test]
    fn input_event_is_24_bytes_with_the_kernel_field_offsets() {
        assert_eq!(std::mem::size_of::<InputEvent>(), 24);
        let ev = InputEvent { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, type_: EV_ABS, code: ABS_X, value: -123 };
        let b = encode(&ev);
        assert_eq!(u16::from_ne_bytes([b[16], b[17]]), EV_ABS);
        assert_eq!(u16::from_ne_bytes([b[18], b[19]]), ABS_X);
        assert_eq!(i32::from_ne_bytes([b[20], b[21], b[22], b[23]]), -123);
    }

    #[test]
    fn struct_sizes_match_the_kernel_abi() {
        assert_eq!(std::mem::size_of::<UinputSetup>(), 92);
        assert_eq!(std::mem::size_of::<UinputAbsSetup>(), 28);
        assert_eq!(std::mem::size_of::<InputAbsinfo>(), 24);
    }

    #[test]
    fn gamepad_value_scales_and_clamps() {
        assert_eq!(gamepad_value(1.0), 32767);
        assert_eq!(gamepad_value(-1.0), -32767);
        assert_eq!(gamepad_value(0.0), 0);
        assert_eq!(gamepad_value(2.0), 32767);
    }

    #[test]
    fn mouse_value_rounds() {
        assert_eq!(mouse_value(11.46), 11);
        assert_eq!(mouse_value(-2.6), -3);
    }

    #[test]
    fn device_names_are_recognisable() {
        assert!(GAMEPAD_NAME.contains("XREAL"));
        assert!(MOUSE_NAME.contains("XREAL"));
    }
}
