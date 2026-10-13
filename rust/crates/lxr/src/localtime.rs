//! The offset of the machine's time zone from UTC, so that words such as "today" and "last week"
//! in a note or a question mean the days of the person who wrote them.
//!
//! Standard Rust has no way to ask for it, so this calls the operating system: `GetTimeZoneInformation`
//! on Windows, `localtime_r` elsewhere on Unix. Both are in the C library the program is linked
//! with already. A machine that cannot answer is read as UTC.

/// Seconds east of UTC of the machine's time zone now (negative to the west), daylight saving time
/// included. 0 when the system does not say.
pub fn utc_offset() -> i64 {
    system_offset().unwrap_or(0)
}

#[cfg(windows)]
fn system_offset() -> Option<i64> {
    /// `SYSTEMTIME`: eight 16-bit fields.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SystemTime([u16; 8]);

    /// `TIME_ZONE_INFORMATION`.
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetTimeZoneInformation(info: *mut TimeZoneInformation) -> u32;
    }

    let mut info = TimeZoneInformation {
        bias: 0,
        standard_name: [0; 32],
        standard_date: SystemTime([0; 8]),
        standard_bias: 0,
        daylight_name: [0; 32],
        daylight_date: SystemTime([0; 8]),
        daylight_bias: 0,
    };
    // SAFETY: `info` has the layout the function writes, and it is not read before the call returns.
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    windows_offset(id, info.bias, info.standard_bias, info.daylight_bias)
}

/// The offset from the values of `TIME_ZONE_INFORMATION`. `id` is what the call returned:
/// 0 (no daylight saving in this zone), 1 (standard time), 2 (daylight time) or the invalid
/// value. The biases are minutes *behind* UTC, so UTC is local time plus the bias.
#[cfg(any(windows, test))]
fn windows_offset(id: u32, bias: i32, standard_bias: i32, daylight_bias: i32) -> Option<i64> {
    let extra = match id {
        0 | 1 => standard_bias,
        2 => daylight_bias,
        _ => return None,
    };
    Some(-(i64::from(bias) + i64::from(extra)) * 60)
}

#[cfg(unix)]
fn system_offset() -> Option<i64> {
    use std::ffi::{c_char, c_int, c_long};

    /// `struct tm` as Linux (glibc and musl), macOS and the BSDs lay it out. Only `gmtoff` is
    /// read; the other fields are there for the layout.
    #[repr(C)]
    #[allow(dead_code)]
    struct Tm {
        sec: c_int,
        min: c_int,
        hour: c_int,
        mday: c_int,
        mon: c_int,
        year: c_int,
        wday: c_int,
        yday: c_int,
        isdst: c_int,
        gmtoff: c_long,
        zone: *const c_char,
    }

    extern "C" {
        fn localtime_r(time: *const c_long, out: *mut Tm) -> *mut Tm;
    }

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?;
    let time = c_long::try_from(now.as_secs()).ok()?;
    let mut out = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    // SAFETY: both pointers are valid for the call, and `localtime_r` keeps no state of its own.
    let done = unsafe { localtime_r(&time, &mut out) };
    (!done.is_null()).then(|| i64::from(out.gmtoff))
}

#[cfg(not(any(windows, unix)))]
fn system_offset() -> Option<i64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_biases_make_the_offset_east_of_utc() {
        // US Central: 360 minutes behind UTC, daylight saving time one hour less.
        assert_eq!(windows_offset(1, 360, 0, -60), Some(-6 * 3600));
        assert_eq!(windows_offset(2, 360, 0, -60), Some(-5 * 3600));
        // India has no daylight saving time: the call says 0, and the bias is 330 minutes east.
        assert_eq!(windows_offset(0, -330, 0, 0), Some(5 * 3600 + 30 * 60));
        // UTC itself.
        assert_eq!(windows_offset(0, 0, 0, 0), Some(0));
        // Central Europe in summer.
        assert_eq!(windows_offset(2, -60, 0, -60), Some(2 * 3600));
        // An invalid answer is no answer.
        assert_eq!(windows_offset(0xFFFF_FFFF, 360, 0, -60), None);
    }

    #[test]
    fn the_machines_offset_is_a_real_time_zone() {
        let offset = utc_offset();
        // Zones run from UTC-12 to UTC+14, in steps of fifteen minutes.
        assert!((-12 * 3600..=14 * 3600).contains(&offset), "{offset}");
        assert_eq!(offset % (15 * 60), 0, "{offset}");
    }
}
