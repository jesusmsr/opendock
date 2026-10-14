//! Low-level HID protocol for AJAZZ / Mirabox "Stream Dock" devices.
//!
//! Every output report is `report_id (0x00)` followed by a fixed-size payload.
//! Command payloads start with `CRT\0\0` and a 3+ letter command:
//!
//! | Command   | Meaning                       | Arguments (payload offsets)         |
//! |-----------|-------------------------------|-------------------------------------|
//! | `DIS`     | wake screens                  | -                                   |
//! | `HAN`     | screens off                   | -                                   |
//! | `CONNECT` | keep-alive                    | -                                   |
//! | `LIG`     | brightness                    | `[10]` = 0..=100                    |
//! | `CLE`     | clear key image               | `[11]` = key id, `0xFF` = all       |
//! | `STP`     | flush (show uploaded images)  | -                                   |
//! | `BAT`     | image header                  | `[8..12]` = JPEG len (BE), `[12]` = key id |
//!
//! After a `BAT` header the JPEG bytes follow in payload-sized chunks.
//! Input reports start with `ACK\0\0OK`; `[9]` = key id, `[10]` = state.

use std::fmt;
use std::time::Duration;

pub use hidapi::HidApi;
use hidapi::{DeviceInfo, HidDevice, HidError};

pub const VID_MIRABOX: u16 = 0x0300;
pub const PID_AKP153E_V3: u16 = 0x3010;
/// Vendor-defined usage page of the control interface.
pub const USAGE_PAGE: u16 = 0xFFA0;

/// Physical layout and image format of the AKP153E (293S V3), confirmed on hardware.
pub mod akp153e {
    pub const ROWS: u8 = 3;
    pub const COLS: u8 = 5;
    /// Key images are square JPEGs of this size.
    pub const IMAGE_SIZE: u32 = 96;
    /// Clockwise rotation to apply to an upright image before upload.
    pub const IMAGE_ROTATION: u16 = 270;

    /// The small display to the right of the keys, as one upright canvas.
    /// It is driven through three image slots (ids 16-18) that each show a
    /// 150 px tall slice; later slots draw over the bottom of earlier ones.
    /// Measured on hardware with tools/probe (`ruler` and `whole`).
    pub mod side {
        pub const WIDTH: u32 = 90;
        pub const HEIGHT: u32 = 394;
        /// Height of the image sent to each slot.
        pub const SLICE_HEIGHT: u32 = 150;
        /// (slot id, first canvas row it shows).
        pub const SLOTS: [(u8, u32); 3] = [(16, 0), (17, 145), (18, 267)];
    }

    /// Device key id for a 0-based (row, col), counted from the top-left.
    /// The device numbers keys down each column, starting top-right.
    pub fn key_id(row: u8, col: u8) -> Option<u8> {
        (row < ROWS && col < COLS).then(|| (COLS - 1 - col) * ROWS + row + 1)
    }

    /// Inverse of [`key_id`].
    pub fn position(id: u8) -> Option<(u8, u8)> {
        let i = id.checked_sub(1)?;
        (i < ROWS * COLS).then(|| (i % ROWS, COLS - 1 - i / ROWS))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn matches_hardware_layout() {
            // Top row, left to right, as observed with tools/probe.
            let top: Vec<u8> = (0..COLS).map(|c| key_id(0, c).unwrap()).collect();
            assert_eq!(top, [13, 10, 7, 4, 1]);
            assert_eq!(key_id(2, 0), Some(15));
            assert_eq!(key_id(3, 0), None);
            for id in 1..=15 {
                let (r, c) = position(id).unwrap();
                assert_eq!(key_id(r, c), Some(id));
            }
            assert_eq!(position(0), None);
            assert_eq!(position(16), None);
        }
    }
}

const CRT: &[u8] = b"CRT\0\0";
/// Payload sizes the device family is known to use, largest first.
const PACKET_SIZES: [usize; 2] = [1024, 512];

#[derive(Debug)]
pub enum Error {
    Hid(HidError),
    NotFound,
    /// No payload size was accepted by the device for writes.
    PacketSize,
    ImageTooLarge(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Hid(e) => write!(f, "HID error: {e}"),
            Error::NotFound => write!(f, "no supported device found"),
            Error::PacketSize => write!(f, "device rejected all known packet sizes"),
            Error::ImageTooLarge(n) => write!(f, "image is {n} bytes, max is 65535"),
        }
    }
}

impl std::error::Error for Error {}

impl From<HidError> for Error {
    fn from(e: HidError) -> Self {
        Error::Hid(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Control interfaces of supported devices currently attached.
pub fn list_devices(api: &HidApi) -> Vec<&DeviceInfo> {
    api.device_list()
        .filter(|d| d.vendor_id() == VID_MIRABOX && d.usage_page() == USAGE_PAGE)
        .collect()
}

/// A decoded input report.
#[derive(Debug, Clone)]
pub struct InputReport {
    pub raw: Vec<u8>,
    /// `Some((key_id, state))` when the report is a key event (`ACK\0\0OK`).
    pub key: Option<(u8, u8)>,
}

pub struct Device {
    hid: HidDevice,
    payload: usize,
    pub product_id: u16,
    pub serial: String,
}

impl Device {
    /// Opens the first supported device, or the one with `serial` if given,
    /// wakes it and detects its packet size.
    pub fn open(api: &HidApi, serial: Option<&str>) -> Result<Self> {
        let info = list_devices(api)
            .into_iter()
            .find(|d| serial.is_none_or(|s| d.serial_number() == Some(s)))
            .ok_or(Error::NotFound)?;
        let hid = info.open_device(api)?;
        let mut dev = Device {
            hid,
            payload: 0,
            product_id: info.product_id(),
            serial: info.serial_number().unwrap_or_default().to_string(),
        };
        dev.detect_packet_size()?;
        Ok(dev)
    }

    /// Payload bytes per output report (excluding the report id).
    pub fn packet_size(&self) -> usize {
        self.payload
    }

    /// Windows rejects writes longer than the interface's output report, so the
    /// first `DIS` (wake) that succeeds tells us the payload size.
    fn detect_packet_size(&mut self) -> Result<()> {
        for size in PACKET_SIZES {
            self.payload = size;
            if self.command(b"DIS", &[]).is_ok() {
                return Ok(());
            }
        }
        Err(Error::PacketSize)
    }

    fn write_payload(&self, payload: &[u8]) -> Result<()> {
        let mut buf = vec![0u8; self.payload + 1];
        buf[1..1 + payload.len()].copy_from_slice(payload);
        self.hid.write(&buf)?;
        Ok(())
    }

    /// Sends `CRT\0\0<cmd>` with `args` written at the given payload offsets.
    fn command(&self, cmd: &[u8], args: &[(usize, u8)]) -> Result<()> {
        let mut p = vec![0u8; self.payload];
        p[..CRT.len()].copy_from_slice(CRT);
        p[CRT.len()..CRT.len() + cmd.len()].copy_from_slice(cmd);
        for &(i, v) in args {
            p[i] = v;
        }
        self.write_payload(&p)
    }

    pub fn wake(&self) -> Result<()> {
        self.command(b"DIS", &[])
    }

    pub fn screen_off(&self) -> Result<()> {
        self.command(b"HAN", &[])
    }

    pub fn keep_alive(&self) -> Result<()> {
        self.command(b"CONNECT", &[])
    }

    pub fn set_brightness(&self, percent: u8) -> Result<()> {
        self.command(b"LIG", &[(10, percent.min(100))])
    }

    pub fn clear_key(&self, key: u8) -> Result<()> {
        self.command(b"CLE", &[(11, key)])
    }

    pub fn clear_all(&self) -> Result<()> {
        self.clear_key(0xFF)
    }

    /// Makes previously uploaded images visible.
    pub fn flush(&self) -> Result<()> {
        self.command(b"STP", &[])
    }

    /// Uploads a JPEG to `key`. Call [`Device::flush`] afterwards.
    pub fn set_key_image(&self, key: u8, jpeg: &[u8]) -> Result<()> {
        let len = jpeg.len();
        if len > 0xFFFF {
            return Err(Error::ImageTooLarge(len));
        }
        let be = (len as u32).to_be_bytes();
        self.command(
            b"BAT",
            &[(8, be[0]), (9, be[1]), (10, be[2]), (11, be[3]), (12, key)],
        )?;
        for chunk in jpeg.chunks(self.payload) {
            self.write_payload(chunk)?;
        }
        Ok(())
    }

    /// Experimental: uploads a JPEG to a rectangle of a secondary screen
    /// (`DRA` command). Header: `[9..12]` = JPEG length (24-bit BE), `[12]` =
    /// screen index, then x, y, width, height as 16-bit BE at `[13..21]`.
    /// Call [`Device::flush`] afterwards.
    pub fn set_area_image(
        &self,
        index: u8,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        jpeg: &[u8],
    ) -> Result<()> {
        let len = jpeg.len();
        if len > 0xFF_FFFF {
            return Err(Error::ImageTooLarge(len));
        }
        let mut args = vec![
            (9, (len >> 16) as u8),
            (10, (len >> 8) as u8),
            (11, len as u8),
            (12, index),
        ];
        for (i, v) in [x, y, w, h].into_iter().enumerate() {
            args.push((13 + 2 * i, (v >> 8) as u8));
            args.push((14 + 2 * i, v as u8));
        }
        self.command(b"DRA", &args)?;
        for chunk in jpeg.chunks(self.payload) {
            self.write_payload(chunk)?;
        }
        Ok(())
    }

    /// Reads one input report, or `None` on timeout.
    pub fn read(&self, timeout: Duration) -> Result<Option<InputReport>> {
        let mut buf = vec![0u8; self.payload + 1];
        let n = self
            .hid
            .read_timeout(&mut buf, timeout.as_millis() as i32)?;
        if n == 0 {
            return Ok(None);
        }
        buf.truncate(n);
        // Some backends keep the 0x00 report id in front; drop it.
        if buf.first() == Some(&0) && buf.get(1) == Some(&b'A') {
            buf.remove(0);
        }
        let key = (buf.len() > 10 && buf.starts_with(b"ACK\0\0OK")).then(|| (buf[9], buf[10]));
        Ok(Some(InputReport { raw: buf, key }))
    }
}
