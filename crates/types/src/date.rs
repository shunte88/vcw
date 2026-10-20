/*
 *  date.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A unix timestamp as a calendar date, which is the only calendar VCW needs.
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

//! A unix timestamp as a calendar date.
//!
//! One function, and it is here rather than in a date crate because this is the
//! whole of VCW's interest in the calendar: a transfer has a date, a project
//! directory has a stamp, and neither needs a time zone database, a locale or a
//! parser. `started_at` is UTC seconds, so the date is the UTC date - which is
//! what an archival record wants anyway, and what every reader of it will assume.

/// Unix seconds as `YYYY-MM-DD`, UTC.
///
/// Hinnant's civil-from-days: shift the epoch to 0000-03-01 so a leap day is the
/// last day of the year rather than a hole in the middle of it, then the 400-year
/// cycle divides exactly and the month length is a line of integer arithmetic
/// rather than a table. Negative seconds are a date before 1970 and come out
/// right, because the shift uses Euclidean division rather than truncation.
#[must_use]
pub fn iso(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::iso;

    /// The epoch, a leap day, the day after one, and a date before 1970.
    ///
    /// The leap day is the case the March-based shift exists for, and the
    /// negative one is the case truncating division gets wrong by a day.
    #[test]
    fn the_calendar_lands_on_the_right_day() {
        assert_eq!(iso(0), "1970-01-01");
        assert_eq!(iso(86_399), "1970-01-01", "still the same day at 23:59:59");
        assert_eq!(iso(951_782_400), "2000-02-29");
        assert_eq!(iso(951_868_800), "2000-03-01");
        assert_eq!(iso(1_760_000_000), "2025-10-09");
        assert_eq!(iso(-1), "1969-12-31");
    }
}
