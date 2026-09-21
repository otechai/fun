//! Calendar dates without a dependency: the C library for local time, plain arithmetic as the fallback.

use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_long};

/// `struct tm` as glibc, musl, and macOS all lay it out.
#[repr(C)]
#[allow(dead_code)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

extern "C" {
    fn tzset();
    fn localtime_r(time: *const c_long, out: *mut Tm) -> *mut Tm;
}

/// `2026-09-20` for a Unix timestamp, in the local time zone (UTC if the C library can't say).
pub fn ymd(secs: u64) -> String {
    let (y, m, d) = local(secs).unwrap_or_else(|| utc(secs));
    format!("{y:04}-{m:02}-{d:02}")
}

fn local(secs: u64) -> Option<(i64, u32, u32)> {
    let time = c_long::try_from(secs).ok()?;
    let mut tm = MaybeUninit::<Tm>::zeroed();
    // SAFETY: `tm` is writable and laid out like the C struct; localtime_r either fills it in
    // completely or returns NULL, and we only read it in the first case.
    unsafe {
        tzset();
        if localtime_r(&time, tm.as_mut_ptr()).is_null() {
            return None;
        }
        let tm = tm.assume_init();
        Some((
            i64::from(tm.tm_year) + 1900,
            u32::try_from(tm.tm_mon + 1).ok()?,
            u32::try_from(tm.tm_mday).ok()?,
        ))
    }
}

/// Days since the epoch -> civil date (Howard Hinnant's algorithm), for the UTC fallback.
fn utc(secs: u64) -> (i64, u32, u32) {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        for (secs, want) in [
            (0, (1970, 1, 1)),
            (86_399, (1970, 1, 1)),
            (86_400, (1970, 1, 2)),
            (951_782_400, (2000, 2, 29)), // a leap day in a century year
            (1_000_000_000, (2001, 9, 9)),
            (1_709_164_800, (2024, 2, 29)),
            (4_102_444_800, (2100, 1, 1)),
        ] {
            assert_eq!(utc(secs), want, "{secs}");
        }
    }

    #[test]
    fn formats_with_padding() {
        // Whatever the machine's zone, mid-year noon UTC is the same calendar day everywhere close to UTC,
        // and the shape is always YYYY-MM-DD.
        let s = ymd(1_000_000_000 - 100_000);
        assert_eq!(s.len(), 10);
        assert_eq!(s.matches('-').count(), 2);
    }
}
