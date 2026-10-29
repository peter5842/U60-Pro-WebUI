//! DRM/KMS output for the QPIC command-mode panel.
//!
//! This follows the path verified on B31 by the community projects (see
//! docs/SCREEN.md): atomic client cap, two dumb RGB565 buffers, and one atomic
//! commit per frame. The first commit also sets the CRTC's MODE_ID/ACTIVE and
//! the connector's CRTC_ID. Never use legacy SETCRTC here: on this panel it
//! times out the QPIC controller and reboots the device.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use crate::canvas::{Canvas, H, W};

const fn ioc(dir: u64, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((b'd' as u64) << 8) | nr
}
const fn iowr<T>(nr: u64) -> u64 {
    ioc(3, nr, std::mem::size_of::<T>())
}
const fn iow<T>(nr: u64) -> u64 {
    ioc(1, nr, std::mem::size_of::<T>())
}

#[repr(C)]
#[derive(Default)]
struct GetCap {
    capability: u64,
    value: u64,
}

#[repr(C)]
#[derive(Default)]
struct CardRes {
    fbs_ptr: u64,
    crtc_id_ptr: u64,
    connector_id_ptr: u64,
    encoder_id_ptr: u64,
    count_fbs: u32,
    count_crtcs: u32,
    count_connectors: u32,
    count_encoders: u32,
    min_width: u32,
    max_width: u32,
    min_height: u32,
    max_height: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct ModeInfo {
    clock: u32,
    hdisplay: u16,
    hsync_start: u16,
    hsync_end: u16,
    htotal: u16,
    hskew: u16,
    vdisplay: u16,
    vsync_start: u16,
    vsync_end: u16,
    vtotal: u16,
    vscan: u16,
    vrefresh: u32,
    flags: u32,
    kind: u32,
    name: [u8; 32],
}

#[repr(C)]
#[derive(Default)]
struct GetConnector {
    encoders_ptr: u64,
    modes_ptr: u64,
    props_ptr: u64,
    prop_values_ptr: u64,
    count_modes: u32,
    count_props: u32,
    count_encoders: u32,
    encoder_id: u32,
    connector_id: u32,
    connector_type: u32,
    connector_type_id: u32,
    connection: u32,
    mm_width: u32,
    mm_height: u32,
    subpixel: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Default)]
struct GetEncoder {
    encoder_id: u32,
    encoder_type: u32,
    crtc_id: u32,
    possible_crtcs: u32,
    possible_clones: u32,
}

#[repr(C)]
#[derive(Default)]
struct PlaneRes {
    plane_id_ptr: u64,
    count_planes: u32,
}

#[repr(C)]
#[derive(Default)]
struct GetPlane {
    plane_id: u32,
    crtc_id: u32,
    fb_id: u32,
    possible_crtcs: u32,
    gamma_size: u32,
    count_format_types: u32,
    format_type_ptr: u64,
}

#[repr(C)]
#[derive(Default)]
struct ObjGetProps {
    props_ptr: u64,
    prop_values_ptr: u64,
    count_props: u32,
    obj_id: u32,
    obj_type: u32,
}

#[repr(C)]
struct GetProperty {
    values_ptr: u64,
    enum_blob_ptr: u64,
    prop_id: u32,
    flags: u32,
    name: [u8; 32],
    count_values: u32,
    count_enum_blobs: u32,
}

#[repr(C)]
#[derive(Default)]
struct CreateBlob {
    data: u64,
    length: u32,
    blob_id: u32,
}

#[repr(C)]
#[derive(Default)]
struct CreateDumb {
    height: u32,
    width: u32,
    bpp: u32,
    flags: u32,
    handle: u32,
    pitch: u32,
    size: u64,
}

#[repr(C)]
#[derive(Default)]
struct MapDumb {
    handle: u32,
    pad: u32,
    offset: u64,
}

#[repr(C)]
#[derive(Default)]
struct FbCmd2 {
    fb_id: u32,
    width: u32,
    height: u32,
    pixel_format: u32,
    flags: u32,
    handles: [u32; 4],
    pitches: [u32; 4],
    offsets: [u32; 4],
    modifier: [u64; 4],
}

#[repr(C)]
#[derive(Default)]
struct Atomic {
    flags: u32,
    count_objs: u32,
    objs_ptr: u64,
    count_props_ptr: u64,
    props_ptr: u64,
    prop_values_ptr: u64,
    reserved: u64,
    user_data: u64,
}

const DRM_IOCTL_GET_CAP: u64 = iowr::<GetCap>(0x0c);
const DRM_IOCTL_SET_CLIENT_CAP: u64 = iow::<GetCap>(0x0d);
const DRM_IOCTL_SET_MASTER: u64 = ioc(0, 0x1e, 0);
const DRM_IOCTL_DROP_MASTER: u64 = ioc(0, 0x1f, 0);
const DRM_IOCTL_MODE_GETRESOURCES: u64 = iowr::<CardRes>(0xA0);
const DRM_IOCTL_MODE_GETENCODER: u64 = iowr::<GetEncoder>(0xA6);
const DRM_IOCTL_MODE_GETCONNECTOR: u64 = iowr::<GetConnector>(0xA7);
const DRM_IOCTL_MODE_GETPROPERTY: u64 = iowr::<GetProperty>(0xAA);
const DRM_IOCTL_MODE_RMFB: u64 = iowr::<u32>(0xAF);
const DRM_IOCTL_MODE_CREATE_DUMB: u64 = iowr::<CreateDumb>(0xB2);
const DRM_IOCTL_MODE_MAP_DUMB: u64 = iowr::<MapDumb>(0xB3);
const DRM_IOCTL_MODE_DESTROY_DUMB: u64 = iowr::<u32>(0xB4);
const DRM_IOCTL_MODE_GETPLANERESOURCES: u64 = iowr::<PlaneRes>(0xB5);
const DRM_IOCTL_MODE_GETPLANE: u64 = iowr::<GetPlane>(0xB6);
const DRM_IOCTL_MODE_ADDFB2: u64 = iowr::<FbCmd2>(0xB8);
const DRM_IOCTL_MODE_OBJ_GETPROPERTIES: u64 = iowr::<ObjGetProps>(0xB9);
const DRM_IOCTL_MODE_ATOMIC: u64 = iowr::<Atomic>(0xBC);
const DRM_IOCTL_MODE_CREATEPROPBLOB: u64 = iowr::<CreateBlob>(0xBD);
const DRM_IOCTL_MODE_DESTROYPROPBLOB: u64 = iowr::<u32>(0xBE);

const DRM_CAP_DUMB_BUFFER: u64 = 1;
const DRM_CLIENT_CAP_ATOMIC: u64 = 3;
const DRM_MODE_OBJECT_CRTC: u32 = 0xcccc_cccc;
const DRM_MODE_OBJECT_CONNECTOR: u32 = 0xc0c0_c0c0;
const DRM_MODE_OBJECT_PLANE: u32 = 0xeeee_eeee;
const DRM_MODE_ATOMIC_ALLOW_MODESET: u32 = 0x0400;
const DRM_MODE_CONNECTED: u32 = 1;
const DRM_FORMAT_RGB565: u32 = u32::from_le_bytes(*b"RG16");

fn ioctl<T>(fd: i32, req: u64, arg: *mut T) -> io::Result<()> {
    loop {
        // SAFETY: every caller passes a pointer to a live #[repr(C)] struct whose
        // layout and size match the request code (sizes are encoded in `req`).
        let r = unsafe { libc::ioctl(fd, req as _, arg as *mut c_void) };
        if r == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) | Some(libc::EAGAIN) => continue,
            _ => return Err(e),
        }
    }
}

fn ptr<T>(v: &mut [T]) -> u64 {
    v.as_mut_ptr() as u64
}

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

struct Buffer {
    handle: u32,
    pitch: u32,
    size: usize,
    fb_id: u32,
    map: *mut u8,
}

#[derive(Default)]
struct Props {
    fb_id: u32,
    crtc_id: u32,
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
    crtc_x: u32,
    crtc_y: u32,
    crtc_w: u32,
    crtc_h: u32,
    mode_id: u32,
    active: u32,
    conn_crtc_id: u32,
}

pub struct Display {
    file: File,
    conn: u32,
    crtc: u32,
    plane: u32,
    mode_blob: u32,
    props: Props,
    bufs: Vec<Buffer>,
    back: usize,
    first: bool,
    pub log: Vec<String>,
}

impl Display {
    pub fn open() -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open("/dev/dri/card0")?;
        let fd = file.as_raw_fd();
        let mut cap = GetCap {
            capability: DRM_CAP_DUMB_BUFFER,
            value: 0,
        };
        ioctl(fd, DRM_IOCTL_GET_CAP, &mut cap)?;
        if cap.value == 0 {
            return Err(err("no dumb buffer support"));
        }
        let mut client = GetCap {
            capability: DRM_CLIENT_CAP_ATOMIC,
            value: 1,
        };
        ioctl(fd, DRM_IOCTL_SET_CLIENT_CAP, &mut client)?;
        let mut d = Display {
            file,
            conn: 0,
            crtc: 0,
            plane: 0,
            mode_blob: 0,
            props: Props::default(),
            bufs: Vec::new(),
            back: 0,
            first: true,
            log: Vec::new(),
        };
        // Master is required to commit. The supervisor has stopped the stock UI,
        // so another holder means something else owns the panel: give up.
        ioctl(fd, DRM_IOCTL_SET_MASTER, std::ptr::null_mut::<u8>())
            .map_err(|e| err(format!("SET_MASTER: {e} (panel owned by another process)")))?;
        let mode = d.find_objects()?;
        d.map_props()?;
        let mut mode = mode;
        let mut blob = CreateBlob {
            data: &mut mode as *mut ModeInfo as u64,
            length: std::mem::size_of::<ModeInfo>() as u32,
            blob_id: 0,
        };
        ioctl(fd, DRM_IOCTL_MODE_CREATEPROPBLOB, &mut blob)?;
        d.mode_blob = blob.blob_id;
        for _ in 0..2 {
            let b = d.create_buffer()?;
            d.bufs.push(b);
        }
        d.log.push(format!(
            "conn={} crtc={} plane={} mode={}x{}@{}",
            d.conn, d.crtc, d.plane, mode.hdisplay, mode.vdisplay, mode.vrefresh
        ));
        Ok(d)
    }

    fn fd(&self) -> i32 {
        self.file.as_raw_fd()
    }

    fn find_objects(&mut self) -> io::Result<ModeInfo> {
        let fd = self.fd();
        let mut res = CardRes::default();
        ioctl(fd, DRM_IOCTL_MODE_GETRESOURCES, &mut res)?;
        if res.count_connectors == 0 || res.count_crtcs == 0 {
            return Err(err("no connector or crtc"));
        }
        let mut conns = vec![0u32; res.count_connectors as usize];
        let mut crtcs = vec![0u32; res.count_crtcs as usize];
        let mut encs = vec![0u32; res.count_encoders.max(1) as usize];
        let mut fbs = vec![0u32; res.count_fbs.max(1) as usize];
        res.connector_id_ptr = ptr(&mut conns);
        res.crtc_id_ptr = ptr(&mut crtcs);
        res.encoder_id_ptr = ptr(&mut encs);
        res.fbs_ptr = ptr(&mut fbs);
        ioctl(fd, DRM_IOCTL_MODE_GETRESOURCES, &mut res)?;
        conns.truncate(res.count_connectors as usize);
        crtcs.truncate(res.count_crtcs as usize);

        for &conn_id in &conns {
            // Probe with room for one mode: without master and count_modes = 0
            // the driver may not report a mode at all.
            let mut probe_mode = [ModeInfo::default()];
            let mut c = GetConnector {
                connector_id: conn_id,
                count_modes: 1,
                modes_ptr: ptr(&mut probe_mode),
                ..Default::default()
            };
            if ioctl(fd, DRM_IOCTL_MODE_GETCONNECTOR, &mut c).is_err() {
                continue;
            }
            if c.connection != DRM_MODE_CONNECTED || c.count_modes == 0 {
                continue;
            }
            let mut modes = vec![ModeInfo::default(); c.count_modes as usize];
            let mut encoders = vec![0u32; c.count_encoders.max(1) as usize];
            let mut props = vec![0u32; c.count_props.max(1) as usize];
            let mut values = vec![0u64; c.count_props.max(1) as usize];
            c.modes_ptr = ptr(&mut modes);
            c.encoders_ptr = ptr(&mut encoders);
            c.props_ptr = ptr(&mut props);
            c.prop_values_ptr = ptr(&mut values);
            ioctl(fd, DRM_IOCTL_MODE_GETCONNECTOR, &mut c)?;
            modes.truncate(c.count_modes.min(modes.len() as u32) as usize);
            let mode = modes
                .iter()
                .find(|m| m.hdisplay as i32 == W && m.vdisplay as i32 == H)
                .or(modes.first())
                .copied()
                .ok_or_else(|| err("connector has no modes"))?;

            let enc_id = if c.encoder_id != 0 {
                c.encoder_id
            } else {
                encoders[0]
            };
            let mut crtc = 0;
            if enc_id != 0 {
                let mut e = GetEncoder {
                    encoder_id: enc_id,
                    ..Default::default()
                };
                if ioctl(fd, DRM_IOCTL_MODE_GETENCODER, &mut e).is_ok() {
                    crtc = if e.crtc_id != 0 {
                        e.crtc_id
                    } else {
                        crtcs
                            .iter()
                            .enumerate()
                            .find(|(i, _)| e.possible_crtcs & (1 << i) != 0)
                            .map(|(_, id)| *id)
                            .unwrap_or(0)
                    };
                }
            }
            if crtc == 0 {
                crtc = crtcs[0];
            }
            let crtc_index = crtcs
                .iter()
                .position(|id| *id == crtc)
                .ok_or_else(|| err("crtc not listed"))?;

            let mut pres = PlaneRes::default();
            ioctl(fd, DRM_IOCTL_MODE_GETPLANERESOURCES, &mut pres)?;
            let mut planes = vec![0u32; pres.count_planes.max(1) as usize];
            pres.plane_id_ptr = ptr(&mut planes);
            ioctl(fd, DRM_IOCTL_MODE_GETPLANERESOURCES, &mut pres)?;
            planes.truncate(pres.count_planes as usize);
            for &plane_id in &planes {
                let mut pl = GetPlane {
                    plane_id,
                    ..Default::default()
                };
                if ioctl(fd, DRM_IOCTL_MODE_GETPLANE, &mut pl).is_err()
                    || pl.possible_crtcs & (1 << crtc_index) == 0
                {
                    continue;
                }
                let mut formats = vec![0u32; pl.count_format_types.max(1) as usize];
                pl.format_type_ptr = ptr(&mut formats);
                if ioctl(fd, DRM_IOCTL_MODE_GETPLANE, &mut pl).is_err() {
                    continue;
                }
                if formats.contains(&DRM_FORMAT_RGB565) {
                    self.conn = c.connector_id;
                    self.crtc = crtc;
                    self.plane = plane_id;
                    return Ok(mode);
                }
            }
        }
        Err(err("no connected connector with an RGB565 plane"))
    }

    fn prop_id(&self, obj: u32, kind: u32, want: &str) -> io::Result<u32> {
        let fd = self.fd();
        let mut q = ObjGetProps {
            obj_id: obj,
            obj_type: kind,
            ..Default::default()
        };
        ioctl(fd, DRM_IOCTL_MODE_OBJ_GETPROPERTIES, &mut q)?;
        let mut ids = vec![0u32; q.count_props.max(1) as usize];
        let mut values = vec![0u64; q.count_props.max(1) as usize];
        q.props_ptr = ptr(&mut ids);
        q.prop_values_ptr = ptr(&mut values);
        ioctl(fd, DRM_IOCTL_MODE_OBJ_GETPROPERTIES, &mut q)?;
        ids.truncate(q.count_props.min(ids.len() as u32) as usize);
        for id in ids {
            let mut p = GetProperty {
                values_ptr: 0,
                enum_blob_ptr: 0,
                prop_id: id,
                flags: 0,
                name: [0; 32],
                count_values: 0,
                count_enum_blobs: 0,
            };
            if ioctl(fd, DRM_IOCTL_MODE_GETPROPERTY, &mut p).is_err() {
                continue;
            }
            let end = p.name.iter().position(|b| *b == 0).unwrap_or(32);
            if &p.name[..end] == want.as_bytes() {
                return Ok(id);
            }
        }
        Err(err(format!("property {want} not found on {obj}")))
    }

    fn map_props(&mut self) -> io::Result<()> {
        let (pl, cr, co) = (self.plane, self.crtc, self.conn);
        let p = DRM_MODE_OBJECT_PLANE;
        self.props = Props {
            fb_id: self.prop_id(pl, p, "FB_ID")?,
            crtc_id: self.prop_id(pl, p, "CRTC_ID")?,
            src_x: self.prop_id(pl, p, "SRC_X")?,
            src_y: self.prop_id(pl, p, "SRC_Y")?,
            src_w: self.prop_id(pl, p, "SRC_W")?,
            src_h: self.prop_id(pl, p, "SRC_H")?,
            crtc_x: self.prop_id(pl, p, "CRTC_X")?,
            crtc_y: self.prop_id(pl, p, "CRTC_Y")?,
            crtc_w: self.prop_id(pl, p, "CRTC_W")?,
            crtc_h: self.prop_id(pl, p, "CRTC_H")?,
            mode_id: self.prop_id(cr, DRM_MODE_OBJECT_CRTC, "MODE_ID")?,
            active: self.prop_id(cr, DRM_MODE_OBJECT_CRTC, "ACTIVE")?,
            conn_crtc_id: self.prop_id(co, DRM_MODE_OBJECT_CONNECTOR, "CRTC_ID")?,
        };
        Ok(())
    }

    fn create_buffer(&mut self) -> io::Result<Buffer> {
        let fd = self.fd();
        let mut c = CreateDumb {
            width: W as u32,
            height: H as u32,
            bpp: 16,
            ..Default::default()
        };
        ioctl(fd, DRM_IOCTL_MODE_CREATE_DUMB, &mut c)?;
        let mut fb = FbCmd2 {
            width: W as u32,
            height: H as u32,
            pixel_format: DRM_FORMAT_RGB565,
            ..Default::default()
        };
        fb.handles[0] = c.handle;
        fb.pitches[0] = c.pitch;
        ioctl(fd, DRM_IOCTL_MODE_ADDFB2, &mut fb)?;
        let mut m = MapDumb {
            handle: c.handle,
            ..Default::default()
        };
        ioctl(fd, DRM_IOCTL_MODE_MAP_DUMB, &mut m)?;
        // SAFETY: mapping the dumb buffer the kernel just created for this fd;
        // the size comes from CREATE_DUMB and the offset from MAP_DUMB.
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                c.size as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                m.offset as libc::off_t,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `map` is a fresh writable mapping of `c.size` bytes.
        unsafe { std::ptr::write_bytes(map as *mut u8, 0, c.size as usize) };
        Ok(Buffer {
            handle: c.handle,
            pitch: c.pitch,
            size: c.size as usize,
            fb_id: fb.fb_id,
            map: map as *mut u8,
        })
    }

    /// Draw `canvas` into the back buffer (rotated for the panel) and commit it.
    pub fn present(&mut self, canvas: &Canvas) -> io::Result<()> {
        let b = &self.bufs[self.back];
        // SAFETY: the mapping lives as long as `self` and is `b.size` bytes long.
        let dst = unsafe { std::slice::from_raw_parts_mut(b.map, b.size) };
        canvas.blit_rotated(dst, b.pitch as usize);
        let fb = b.fb_id;
        self.commit(fb)?;
        self.back ^= 1;
        Ok(())
    }

    fn commit(&mut self, fb: u32) -> io::Result<()> {
        let p = &self.props;
        let mut objs = Vec::with_capacity(13);
        let mut props = Vec::with_capacity(13);
        let mut values: Vec<u64> = Vec::with_capacity(13);
        let mut add = |obj: u32, prop: u32, value: u64| {
            objs.push(obj);
            props.push(prop);
            values.push(value);
        };
        let (w, h) = (W as u64, H as u64);
        add(self.plane, p.fb_id, fb as u64);
        add(self.plane, p.crtc_id, self.crtc as u64);
        add(self.plane, p.src_x, 0);
        add(self.plane, p.src_y, 0);
        add(self.plane, p.src_w, w << 16);
        add(self.plane, p.src_h, h << 16);
        add(self.plane, p.crtc_x, 0);
        add(self.plane, p.crtc_y, 0);
        add(self.plane, p.crtc_w, w);
        add(self.plane, p.crtc_h, h);
        let mut flags = 0;
        if self.first {
            add(self.crtc, p.active, 1);
            add(self.crtc, p.mode_id, self.mode_blob as u64);
            add(self.conn, p.conn_crtc_id, self.crtc as u64);
            flags |= DRM_MODE_ATOMIC_ALLOW_MODESET;
        }
        let mut counts = vec![1u32; objs.len()];
        let mut a = Atomic {
            flags,
            count_objs: objs.len() as u32,
            objs_ptr: ptr(&mut objs),
            count_props_ptr: ptr(&mut counts),
            props_ptr: ptr(&mut props),
            prop_values_ptr: ptr(&mut values),
            ..Default::default()
        };
        ioctl(self.fd(), DRM_IOCTL_MODE_ATOMIC, &mut a)?;
        self.first = false;
        Ok(())
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        let fd = self.fd();
        for b in self.bufs.drain(..) {
            // SAFETY: unmapping the mapping created in create_buffer.
            unsafe { libc::munmap(b.map as *mut c_void, b.size) };
            let mut id = b.fb_id;
            let _ = ioctl(fd, DRM_IOCTL_MODE_RMFB, &mut id);
            let mut handle = b.handle;
            let _ = ioctl(fd, DRM_IOCTL_MODE_DESTROY_DUMB, &mut handle);
        }
        if self.mode_blob != 0 {
            let mut id = self.mode_blob;
            let _ = ioctl(fd, DRM_IOCTL_MODE_DESTROYPROPBLOB, &mut id);
        }
        let _ = ioctl(fd, DRM_IOCTL_DROP_MASTER, std::ptr::null_mut::<u8>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_the_uapi() {
        // Sizes from include/uapi/drm/drm_mode.h on aarch64.
        assert_eq!(std::mem::size_of::<ModeInfo>(), 68);
        assert_eq!(std::mem::size_of::<CardRes>(), 64);
        assert_eq!(std::mem::size_of::<GetConnector>(), 80);
        assert_eq!(std::mem::size_of::<GetEncoder>(), 20);
        assert_eq!(std::mem::size_of::<PlaneRes>(), 16);
        assert_eq!(std::mem::size_of::<GetPlane>(), 32);
        assert_eq!(std::mem::size_of::<ObjGetProps>(), 32);
        assert_eq!(std::mem::size_of::<GetProperty>(), 64);
        assert_eq!(std::mem::size_of::<CreateBlob>(), 16);
        assert_eq!(std::mem::size_of::<CreateDumb>(), 32);
        assert_eq!(std::mem::size_of::<MapDumb>(), 16);
        assert_eq!(std::mem::size_of::<FbCmd2>(), 104);
        assert_eq!(std::mem::size_of::<Atomic>(), 56);
    }

    #[test]
    fn request_codes_match_libdrm() {
        assert_eq!(DRM_IOCTL_MODE_ATOMIC, 0xc038_64bc);
        assert_eq!(DRM_IOCTL_MODE_ADDFB2, 0xc068_64b8);
        assert_eq!(DRM_IOCTL_MODE_GETRESOURCES, 0xc040_64a0);
        assert_eq!(DRM_IOCTL_MODE_GETCONNECTOR, 0xc050_64a7);
        assert_eq!(DRM_IOCTL_SET_CLIENT_CAP, 0x4010_640d);
        assert_eq!(DRM_IOCTL_SET_MASTER, 0x641e);
        assert_eq!(DRM_IOCTL_MODE_CREATE_DUMB, 0xc020_64b2);
        assert_eq!(DRM_FORMAT_RGB565, 0x3631_4752);
    }
}
