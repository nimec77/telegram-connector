//! UTC instants with the connector's RFC 3339 wire representation.

use jiff::SignedDuration;
use jiff::civil::{DateTime, date};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt;
use std::ops::{Add, Sub};
use std::str::FromStr;
use std::time::{Duration, SystemTime};

const NANOS_PER_SECOND: i128 = 1_000_000_000;
const EPOCH: DateTime = date(1970, 1, 1).at(0, 0, 0, 0);

/// A UTC instant serialized as an RFC 3339 string with a `Z` suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTimestamp {
    seconds: i64,
    // RFC 3339 leap seconds are stored after the preceding second's ordinary fraction.
    nanoseconds: u32,
}

/// An invalid or out-of-range RFC 3339 UTC instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid RFC 3339 timestamp")]
pub struct InvalidTimestamp;

impl UtcTimestamp {
    /// The current wall-clock time.
    pub fn now() -> Self {
        SystemTime::now()
            .try_into()
            .expect("current time within the supported calendar")
    }

    /// Construct a whole-second instant from a Unix timestamp.
    ///
    /// # Errors
    /// Returns an error if the seconds fall outside the supported timestamp range.
    pub fn from_unix_seconds(seconds: i64) -> Result<Self, InvalidTimestamp> {
        let timestamp = Self {
            seconds,
            nanoseconds: 0,
        };
        timestamp.civil_datetime().map(|_| timestamp)
    }

    /// Parse an RFC 3339 filter value with a four-digit year.
    ///
    /// # Errors
    /// Returns an error for malformed input or dates outside the supported range.
    pub fn parse_rfc3339(input: &str) -> Result<Self, InvalidTimestamp> {
        if !input.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return Err(InvalidTimestamp);
        }
        input.parse()
    }

    /// Whole Unix seconds, rounded down for fractional pre-epoch instants.
    pub const fn unix_seconds(self) -> i64 {
        self.seconds
    }

    /// Add an elapsed duration when the result fits the timestamp range.
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.shift(duration.as_nanos() as i128).ok()
    }

    /// Format in RFC 3339 with an explicit `+00:00` offset, as used by legacy responses.
    pub fn to_rfc3339(self) -> String {
        format!("{}", self.rfc3339("+00:00", true))
    }

    /// Format UTC at whole-second precision, as used by sending receipts.
    pub fn to_rfc3339_seconds(self) -> String {
        format!("{}", self.rfc3339("Z", false))
    }

    fn from_nanoseconds(nanos: i128) -> Result<Self, InvalidTimestamp> {
        let seconds =
            i64::try_from(nanos.div_euclid(NANOS_PER_SECOND)).map_err(|_| InvalidTimestamp)?;
        let mut timestamp = Self::from_unix_seconds(seconds)?;
        timestamp.nanoseconds = nanos.rem_euclid(NANOS_PER_SECOND) as u32;
        Ok(timestamp)
    }

    fn civil_datetime(self) -> Result<DateTime, InvalidTimestamp> {
        EPOCH
            .checked_add(SignedDuration::from_secs(self.seconds))
            .map_err(|_| InvalidTimestamp)
    }

    fn is_leap_second(self) -> bool {
        self.nanoseconds >= NANOS_PER_SECOND as u32
    }

    fn shift(self, offset: i128) -> Result<Self, InvalidTimestamp> {
        if self.is_leap_second() {
            let fraction = i128::from(self.nanoseconds) + offset;
            if (NANOS_PER_SECOND..2 * NANOS_PER_SECOND).contains(&fraction) {
                return Ok(Self {
                    seconds: self.seconds,
                    nanoseconds: fraction as u32,
                });
            }
            let carried = if fraction >= 2 * NANOS_PER_SECOND {
                fraction - NANOS_PER_SECOND
            } else {
                fraction
            };
            return Self::from_nanoseconds(i128::from(self.seconds) * NANOS_PER_SECOND + carried);
        }
        Self::from_nanoseconds(
            i128::from(self.seconds) * NANOS_PER_SECOND + i128::from(self.nanoseconds) + offset,
        )
    }

    fn saturating_shift(self, offset: i128) -> Self {
        self.shift(offset).unwrap_or_else(|_| {
            let boundary = if offset < 0 {
                DateTime::MIN
            } else {
                DateTime::MAX
            };
            Self::from_nanoseconds(boundary.duration_since(EPOCH).as_nanos())
                .expect("civil timestamp boundary")
        })
    }

    fn rfc3339(self, suffix: &str, fractional: bool) -> impl fmt::Display + '_ {
        fmt::from_fn(move |f| {
            let utc = self.civil_datetime().map_err(|_| fmt::Error)?;
            let year = i32::from(utc.year());
            if year < 0 {
                f.write_str("-")?;
            }
            let leap = self.is_leap_second();
            let nanos = self.nanoseconds % NANOS_PER_SECOND as u32;
            write!(
                f,
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                year.abs(),
                utc.month(),
                utc.day(),
                utc.hour(),
                utc.minute(),
                utc.second() + i8::from(leap)
            )?;
            if fractional && nanos != 0 {
                if nanos.is_multiple_of(1_000_000) {
                    write!(f, ".{:03}", nanos / 1_000_000)?;
                } else if nanos.is_multiple_of(1_000) {
                    write!(f, ".{:06}", nanos / 1_000)?;
                } else {
                    write!(f, ".{nanos:09}")?;
                }
            }
            f.write_str(suffix)
        })
    }
}

impl FromStr for UtcTimestamp {
    type Err = InvalidTimestamp;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let year_len = if matches!(input.as_bytes().first(), Some(b'-' | b'+')) {
            input[1..]
                .find('-')
                .map(|offset| offset + 1)
                .filter(|len| matches!(len, 5 | 7))
                .ok_or(InvalidTimestamp)?
        } else {
            4
        };
        let (year, rest) = input.split_at_checked(year_len).ok_or(InvalidTimestamp)?;
        let year: i16 = year.parse().map_err(|_| InvalidTimestamp)?;
        let (clock, tail) = rest.split_at_checked(15).ok_or(InvalidTimestamp)?;
        if !clock.bytes().enumerate().all(|(i, b)| match i {
            0 | 3 => b == b'-',
            6 => matches!(b, b'T' | b't' | b' '),
            9 | 12 => b == b':',
            _ => b.is_ascii_digit(),
        }) {
            return Err(InvalidTimestamp);
        }
        let two_digits = |index: usize| {
            (clock.as_bytes()[index] - b'0') as i8 * 10 + (clock.as_bytes()[index + 1] - b'0') as i8
        };
        let second = two_digits(13);
        let civil = DateTime::new(
            year,
            two_digits(1),
            two_digits(4),
            two_digits(7),
            two_digits(10),
            if second == 60 { 59 } else { second },
            0,
        )
        .map_err(|_| InvalidTimestamp)?;
        let (nanoseconds, offset) = if let Some(digits) = tail.strip_prefix('.') {
            let digits_len = digits.bytes().take_while(u8::is_ascii_digit).count();
            let fraction = &digits[..digits_len.min(9)];
            let value: u32 = fraction.parse().map_err(|_| InvalidTimestamp)?;
            (
                value * 10_u32.pow((9 - fraction.len()) as u32),
                &digits[digits_len..],
            )
        } else {
            (0, tail)
        };
        let seconds = civil.duration_since(EPOCH).as_secs() - offset_seconds(offset)?;
        let mut timestamp = Self::from_unix_seconds(seconds)?;
        timestamp.nanoseconds = nanoseconds + u32::from(second == 60) * NANOS_PER_SECOND as u32;
        Ok(timestamp)
    }
}

fn offset_seconds(offset: &str) -> Result<i64, InvalidTimestamp> {
    if matches!(offset, "Z" | "z") {
        return Ok(0);
    }
    let (sign, time) = if let Some(time) = offset.strip_prefix('+') {
        (1, time)
    } else {
        (
            -1,
            offset
                .strip_prefix('-')
                .or_else(|| offset.strip_prefix('−'))
                .ok_or(InvalidTimestamp)?,
        )
    };
    if !matches!(
        time.as_bytes(),
        [b'0'..=b'2', b'0'..=b'9', b':', b'0'..=b'5', b'0'..=b'9']
    ) || &time[..2] > "23"
    {
        return Err(InvalidTimestamp);
    }
    let hours: i64 = time[..2].parse().map_err(|_| InvalidTimestamp)?;
    let minutes: i64 = time[3..].parse().map_err(|_| InvalidTimestamp)?;
    Ok(sign * (hours * 3600 + minutes * 60))
}

impl TryFrom<SystemTime> for UtcTimestamp {
    type Error = InvalidTimestamp;

    fn try_from(value: SystemTime) -> Result<Self, Self::Error> {
        let nanos = match value.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos() as i128,
            Err(error) => -(error.duration().as_nanos() as i128),
        };
        Self::from_nanoseconds(nanos)
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.rfc3339("Z", true).fmt(f)
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = Cow::<'de, str>::deserialize(deserializer)?;
        let parsed = if let Some(utc) = input.strip_suffix(" UTC") {
            format!("{utc}Z").parse()
        } else if let Some((prefix, offset)) = input
            .len()
            .checked_sub(5)
            .and_then(|i| input.split_at_checked(i))
            && matches!(
                offset.as_bytes(),
                [
                    b'+' | b'-',
                    b'0'..=b'9',
                    b'0'..=b'9',
                    b'0'..=b'9',
                    b'0'..=b'9'
                ]
            )
        {
            format!("{prefix}{}:{}", &offset[..3], &offset[3..]).parse()
        } else {
            input.parse()
        };
        parsed.map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for UtcTimestamp {
    fn inline_schema() -> bool {
        true
    }
    fn schema_name() -> Cow<'static, str> {
        "UtcTimestamp".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "date-time"})
    }
}

impl Add<SignedDuration> for UtcTimestamp {
    type Output = Self;
    fn add(self, rhs: SignedDuration) -> Self {
        self.saturating_shift(rhs.as_nanos())
    }
}

impl Sub<SignedDuration> for UtcTimestamp {
    type Output = Self;
    fn sub(self, rhs: SignedDuration) -> Self {
        self.saturating_shift(-rhs.as_nanos())
    }
}

impl Sub for UtcTimestamp {
    type Output = SignedDuration;
    fn sub(self, rhs: Self) -> SignedDuration {
        // Keep the legacy same-day time-of-day adjustment for a leap endpoint.
        let lhs_clock = self.seconds.rem_euclid(86_400);
        let rhs_clock = rhs.seconds.rem_euclid(86_400);
        let extra = i64::from(lhs_clock > rhs_clock && rhs.is_leap_second())
            - i64::from(lhs_clock < rhs_clock && self.is_leap_second());
        SignedDuration::new(
            self.seconds - rhs.seconds + extra,
            self.nanoseconds as i32 - rhs.nanoseconds as i32,
        )
    }
}
