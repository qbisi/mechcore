//! The one clock a match reads, written the way a turn file states it.
//!
//! `docs/spec/mechcore/turn.md` says `opened` is an RFC 3339 instant in UTC,
//! and the only question asked of it is how long ago it was. Two conversions
//! answer that: one writes the instant a round opened, the other reads it back.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Now, as a turn file writes it.
#[must_use]
pub(crate) fn now() -> String {
    text(SystemTime::now())
}

/// One instant, as a turn file writes it: `2026-09-20T01:02:03.123456Z`.
///
/// An instant before 1970 cannot be reached by a round that has opened, and
/// the epoch stands in for one rather than failing a write.
fn text(at: SystemTime) -> String {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let seconds = since.as_secs();
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60,
        since.subsec_micros(),
    )
}

/// How many seconds have passed since `stated`, which is what the deployment
/// clock is asked.
///
/// A clock that has gone backwards answers zero rather than a negative age: a
/// round has not opened in the future, and a match should not be ended by a
/// system clock that was set back.
///
/// # Errors
///
/// Returns an error when the instant is not the one this module writes.
pub(crate) fn elapsed(stated: &str) -> Result<f64, String> {
    let at = read(stated)?;
    Ok(SystemTime::now()
        .duration_since(at)
        .map_or(0.0, |since| since.as_secs_f64()))
}

/// Reads back what [`text`] wrote, and nothing else.
fn read(stated: &str) -> Result<SystemTime, String> {
    let unreadable = || format!("{stated:?} is not an instant this platform writes");
    let rest = stated.strip_suffix('Z').ok_or_else(unreadable)?;
    let (date, time) = rest.split_once('T').ok_or_else(unreadable)?;
    let (time, fraction) = match time.split_once('.') {
        Some((time, fraction)) => (time, fraction),
        None => (time, "0"),
    };
    let number = |field: &str, width: usize| -> Result<u64, String> {
        if field.len() != width || !field.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(unreadable());
        }
        field.parse().map_err(|_| unreadable())
    };
    let [year, month, day] = fields(date, '-').ok_or_else(unreadable)?;
    let [hour, minute, second] = fields(time, ':').ok_or_else(unreadable)?;
    let days = days_from_civil(number(year, 4)?, number(month, 2)?, number(day, 2)?)
        .ok_or_else(unreadable)?;
    let micros: u64 = number(fraction, fraction.len())?;
    let scale = 10u64.pow(u32::try_from(fraction.len()).map_err(|_| unreadable())?);
    if scale > 1_000_000 {
        return Err(unreadable());
    }
    Ok(UNIX_EPOCH
        + Duration::from_secs(
            days * 86_400 + number(hour, 2)? * 3600 + number(minute, 2)? * 60 + number(second, 2)?,
        )
        + Duration::from_micros(micros * (1_000_000 / scale)))
}

fn fields(text: &str, between: char) -> Option<[&str; 3]> {
    let mut parts = text.split(between);
    let three = [parts.next()?, parts.next()?, parts.next()?];
    parts.next().is_none().then_some(three)
}

/// Days since the epoch as a year, a month and a day, which is Howard
/// Hinnant's `civil_from_days` with the era shifted to 1970.
fn civil(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let of_era = shifted % 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (year + u64::from(month <= 2), month, day)
}

/// [`civil`]'s other direction, refusing a date that is not one.
fn days_from_civil(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year - u64::from(month <= 2);
    let era = year / 400;
    let year_of_era = year % 400;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146_097 + of_era).checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::{days_from_civil, read, text};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// What one side writes, the other side reads: a turn file is written by
    /// one process and read by the other, so the two halves have to meet.
    #[test]
    fn an_instant_reads_back_as_itself() {
        for seconds in [0, 1, 951_782_400, 1_789_000_000, 4_102_444_800] {
            for micros in [0, 1, 999_999] {
                let at = UNIX_EPOCH + Duration::from_secs(seconds) + Duration::from_micros(micros);
                assert_eq!(read(&text(at)).unwrap(), at, "{}", text(at));
            }
        }
        let now = SystemTime::now();
        assert!(read(&text(now)).unwrap() <= now);
    }

    #[test]
    fn a_known_instant_is_written_the_way_the_contract_states_it() {
        let at = UNIX_EPOCH + Duration::from_secs(1_789_866_123) + Duration::from_micros(123_456);
        assert_eq!(text(at), "2026-09-20T01:02:03.123456Z");
    }

    /// A leap day is a date the shifted era has to get right, and the last
    /// day of a century is where a wrong one lands.
    #[test]
    fn leap_days_and_century_ends_round_trip() {
        for (year, month, day) in [
            (2000, 2, 29),
            (2024, 2, 29),
            (2100, 2, 28),
            (1970, 1, 1),
            (2026, 12, 31),
        ] {
            let days = days_from_civil(year, month, day).unwrap();
            assert_eq!(
                super::civil(days),
                (year, month, day),
                "{year}-{month}-{day}"
            );
        }
    }

    /// Anything but the shape this module writes is refused, because a turn
    /// file that says something else is a file this platform did not write.
    #[test]
    fn another_spelling_is_not_an_instant() {
        for stated in [
            "",
            "2026-09-20",
            "2026-09-20T01:02:03",
            "2026-09-20T01:02:03+01:00",
            "2026-9-20T01:02:03Z",
            "2026-13-20T01:02:03.000000Z",
            "2026-09-20T01:02:03.1234567Z",
            "now",
        ] {
            assert!(read(stated).is_err(), "{stated}");
        }
    }
}
