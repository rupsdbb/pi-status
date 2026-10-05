// SPDX-License-Identifier: GPL-3.0-or-later
//! Waveshare UPS HAT (E) over I²C. Register map:
//!   0x02  1 byte   charge state flags
//!   0x10  6 bytes  input mV, mA, mW
//!   0x20 12 bytes  battery mV, mA (signed), %, remaining mAh, min to empty, min to full
//!   0x30  8 bytes  cell 1-4 mV

use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;

const I2C_RDWR: u64 = 0x0707;
const I2C_M_RD: u16 = 0x0001;

#[repr(C)]
struct I2cMsg {
    addr: u16,
    flags: u16,
    len: u16,
    buf: *mut u8,
}

#[repr(C)]
struct I2cRdwrData {
    msgs: *mut I2cMsg,
    nmsgs: u32,
}

/// Write the register address, then read `len` bytes (repeated start),
/// equivalent to smbus `read_i2c_block_data`.
fn read_block(f: &File, addr: u16, reg: u8, len: usize) -> io::Result<Vec<u8>> {
    let mut reg_buf = [reg];
    let mut out = vec![0u8; len];
    let mut msgs = [
        I2cMsg { addr, flags: 0, len: 1, buf: reg_buf.as_mut_ptr() },
        I2cMsg { addr, flags: I2C_M_RD, len: len as u16, buf: out.as_mut_ptr() },
    ];
    let mut data = I2cRdwrData { msgs: msgs.as_mut_ptr(), nmsgs: msgs.len() as u32 };
    let r = unsafe { libc::ioctl(f.as_raw_fd(), I2C_RDWR as _, &mut data as *mut I2cRdwrData) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(out)
    }
}

fn u16le(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([d[i], d[i + 1]])
}

#[derive(Serialize, Clone)]
pub struct Ups {
    pub state: String,
    pub on_battery: bool,
    pub input_mv: u32,
    pub input_ma: u32,
    pub input_mw: u32,
    pub battery_mv: u32,
    /// Negative while discharging.
    pub battery_ma: i32,
    pub percent: u32,
    pub remaining_mah: u32,
    pub minutes_to_empty: Option<u32>,
    pub minutes_to_full: Option<u32>,
    pub cells_mv: Vec<u32>,
}

pub fn read(bus: u8, address: u16) -> Result<Ups, String> {
    let dev = format!("/dev/i2c-{bus}");
    let f = OpenOptions::new().read(true).write(true).open(&dev).map_err(|e| format!("{dev}: {e}"))?;
    let rd = |reg: u8, len: usize| read_block(&f, address, reg, len).map_err(|e| format!("i2c 0x{address:02x} reg 0x{reg:02x}: {e}"));

    let flags = rd(0x02, 1)?[0];
    let state = if flags & 0x40 != 0 {
        "Fast charging"
    } else if flags & 0x80 != 0 {
        "Charging"
    } else if flags & 0x20 != 0 {
        "Discharging"
    } else {
        "Idle"
    };

    let input = rd(0x10, 6)?;
    let batt = rd(0x20, 12)?;
    let cells = rd(0x30, 8)?;

    let battery_ma = u16le(&batt, 2) as i16 as i32;
    Ok(Ups {
        state: state.into(),
        on_battery: flags & 0x20 != 0 && flags & 0xC0 == 0,
        input_mv: u16le(&input, 0).into(),
        input_ma: u16le(&input, 2).into(),
        input_mw: u16le(&input, 4).into(),
        battery_mv: u16le(&batt, 0).into(),
        battery_ma,
        percent: u16le(&batt, 4).into(),
        remaining_mah: u16le(&batt, 6).into(),
        minutes_to_empty: (battery_ma < 0).then(|| u16le(&batt, 8).into()),
        minutes_to_full: (battery_ma > 0).then(|| u16le(&batt, 10).into()),
        cells_mv: (0..4).map(|i| u16le(&cells, i * 2).into()).collect(),
    })
}
