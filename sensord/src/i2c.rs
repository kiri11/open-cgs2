//! The sensors on the first I2C bus, driven from userspace through i2c-dev, so that the waits
//! while they measure are sleeps rather than the vendor drivers' busy loops. The drivers stay
//! bound for stock mode, unused.
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::thread::sleep;
use std::time::Duration;

const BUS: &str = "/dev/i2c-1";
const I2C_SLAVE_FORCE: libc::c_ulong = 0x0706;
pub const SHT: u16 = 0x44;
pub const OPT: u16 = 0x45;
pub const SGP: u16 = 0x59;
pub const SCD: u16 = 0x62;

/// Sensirion's CRC-8 over each 16-bit word.
fn crc(data: &[u8]) -> u8 {
    data.iter().fold(0xff, |mut c, b| {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 { (c << 1) ^ 0x31 } else { c << 1 };
        }
        c
    })
}

fn ms(n: u64) {
    sleep(Duration::from_millis(n));
}

pub struct Dev(File);

impl Dev {
    /// Forced, since the vendor driver may still be bound to the address.
    pub fn open(addr: u16) -> io::Result<Dev> {
        let f = OpenOptions::new().read(true).write(true).open(BUS)?;
        if unsafe { libc::ioctl(f.as_raw_fd(), I2C_SLAVE_FORCE as _, addr as libc::c_ulong) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Dev(f))
    }

    /// Sends a Sensirion command with its arguments, each word followed by its CRC.
    fn command(&mut self, cmd: &[u8], args: &[u16]) -> io::Result<()> {
        let mut b = cmd.to_vec();
        for a in args {
            let w = a.to_be_bytes();
            b.extend(w);
            b.push(crc(&w));
        }
        self.0.write_all(&b)
    }

    /// Reads Sensirion words, checking each one's CRC.
    fn words(&mut self, n: usize) -> io::Result<Vec<u16>> {
        let mut b = vec![0; n * 3];
        self.0.read_exact(&mut b)?;
        b.chunks(3)
            .map(|c| match crc(&c[..2]) == c[2] {
                true => Ok(u16::from_be_bytes([c[0], c[1]])),
                false => Err(io::Error::new(io::ErrorKind::InvalidData, "CRC mismatch")),
            })
            .collect()
    }

    fn query(&mut self, cmd: u16, wait: u64, n: usize) -> io::Result<Vec<u16>> {
        self.command(&cmd.to_be_bytes(), &[])?;
        ms(wait);
        self.words(n)
    }

    /// A 16-bit register of the OPT3004.
    fn register(&mut self, reg: u8) -> io::Result<u16> {
        self.0.write_all(&[reg])?;
        let mut b = [0; 2];
        self.0.read_exact(&mut b)?;
        Ok(u16::from_be_bytes(b))
    }
}

fn celsius(w: u16) -> f32 {
    -45.0 + 175.0 * w as f32 / 65535.0
}

/// SHT4x temperature (°C) and relative humidity (%), measured at high repeatability.
pub fn sht(d: &mut Dev) -> io::Result<(f32, f32)> {
    d.command(&[0xfd], &[])?;
    ms(10);
    let w = d.words(2)?;
    Ok((celsius(w[0]), (-6.0 + 125.0 * w[1] as f32 / 65535.0).clamp(0.0, 100.0)))
}

/// OPT3004 illuminance (lux) from a single conversion, keeping the configured range and time.
pub fn lux(d: &mut Dev) -> io::Result<f32> {
    let c = (d.register(0x01)? & !0x0600) | 0x0200;
    d.0.write_all(&[0x01, (c >> 8) as u8, c as u8])?;
    ms(if c & 0x0800 != 0 { 880 } else { 110 });
    let r = d.register(0x00)?;
    Ok(0.01 * (1u32 << (r >> 12)) as f32 * (r & 0xfff) as f32)
}

/// SGP40 raw VOC signal, compensated for the given humidity (%) and temperature (°C).
pub fn sgp(d: &mut Dev, rh: f32, t: f32) -> io::Result<f32> {
    let rh = (rh.clamp(0.0, 100.0) * 65535.0 / 100.0).round() as u16;
    let t = ((t.clamp(-45.0, 130.0) + 45.0) * 65535.0 / 175.0).round() as u16;
    d.command(&[0x26, 0x0f], &[rh, t])?;
    ms(30);
    Ok(d.words(1)?[0] as f32)
}

/// Whether the SCD40 has a sample waiting.
pub fn scd_ready(d: &mut Dev) -> io::Result<bool> {
    Ok(d.query(0xe4b8, 1, 1)?[0] & 0x07ff != 0)
}

/// The SCD40's waiting sample: CO2 (ppm), temperature (°C) and relative humidity (%).
pub fn scd(d: &mut Dev) -> io::Result<(f32, f32, f32)> {
    let w = d.query(0xec05, 1, 3)?;
    Ok((w[0] as f32, celsius(w[1]), 100.0 * w[2] as f32 / 65535.0))
}

/// Restarts the SCD40 after a stall: stops it, reloads its settings and starts it again.
pub fn scd_restart(d: &mut Dev) -> io::Result<()> {
    d.command(&[0x3f, 0x86], &[])?;
    ms(500);
    d.command(&[0x36, 0x46], &[])?;
    ms(30);
    d.command(&[0x21, 0xb1], &[])
}

/// Forced recalibration of the SCD40 to `ppm`, the level of the air it has measured for the last
/// 3 minutes. Returns the correction it made, in ppm; its measurements restart afterwards.
pub fn scd_frc(d: &mut Dev, ppm: u16) -> io::Result<i32> {
    d.command(&[0x3f, 0x86], &[])?;
    ms(500);
    let res = (|| {
        d.command(&[0x36, 0x2f], &[ppm])?;
        ms(400);
        match d.words(1)?[0] {
            0xffff => Err(io::Error::new(io::ErrorKind::Other, "the sensor refused")),
            w => Ok(w as i32 - 0x8000),
        }
    })();
    d.command(&[0x21, 0xb1], &[])?;
    res
}

/// (Re)starts the SCD40's measurements, which it has to stop to answer, and returns what the
/// sensors say about themselves, for the log. The first CO2 sample comes 5 s later.
pub fn start() -> String {
    let mut out = Vec::new();
    let mut note = |what: &str, r: io::Result<String>| out.push(format!("{what} {}", r.unwrap_or_else(|e| format!("error: {e}"))));
    note(
        "SHT4x serial",
        Dev::open(SHT).and_then(|mut d| {
            d.command(&[0x89], &[])?;
            ms(1);
            let w = d.words(2)?;
            Ok(format!("{:04x}{:04x}", w[0], w[1]))
        }),
    );
    note(
        "OPT3004",
        Dev::open(OPT).and_then(|mut d| Ok(format!("config {:04x} id {:04x}/{:04x}", d.register(0x01)?, d.register(0x7e)?, d.register(0x7f)?))),
    );
    note(
        "SCD40",
        Dev::open(SCD).and_then(|mut d| {
            d.command(&[0x3f, 0x86], &[])?;
            ms(500);
            let res = (|| {
                let sn = d.query(0x3682, 1, 3)?;
                let offset = d.query(0x2318, 1, 1)?[0] as f32 * 175.0 / 65535.0;
                let altitude = d.query(0x2322, 1, 1)?[0];
                let asc = d.query(0x2313, 1, 1)?[0];
                Ok(format!("serial {:04x}{:04x}{:04x} temperature offset {offset:.2} altitude {altitude} ASC {asc}", sn[0], sn[1], sn[2]))
            })();
            d.command(&[0x21, 0xb1], &[])?;
            res
        }),
    );
    out.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensirion_crc() {
        assert_eq!(crc(&[0xbe, 0xef]), 0x92);
    }
}
