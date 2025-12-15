//! Systemd time span parsing
//!
//! This module provides functionality to parse systemd time span values
//! as used in directives like `RuntimeMaxSec=`, `TimeoutStartSec=`, etc.

use std::time::Duration;

/// Parse a systemd time span string into a Duration
///
/// Systemd accepts time spans in the following formats:
/// - Plain numbers are interpreted as seconds (e.g., `30` = 30 seconds)
/// - Numbers with units: `s` (seconds), `min` (minutes), `h` (hours),
///   `d` (days), `w` (weeks), `ms` (milliseconds), `us` (microseconds)
/// - Multiple values can be combined additively: `2min 30s` = 150 seconds
/// - Whitespace between values is optional: `2min30s` = 150 seconds
///
/// # Example
///
/// ```
/// # use systemd_unit_edit::parse_timespan;
/// # use std::time::Duration;
/// assert_eq!(parse_timespan("30"), Ok(Duration::from_secs(30)));
/// assert_eq!(parse_timespan("2min"), Ok(Duration::from_secs(120)));
/// assert_eq!(parse_timespan("1h 30min"), Ok(Duration::from_secs(5400)));
/// assert_eq!(parse_timespan("2min 30s"), Ok(Duration::from_millis(150_000)));
/// ```
pub fn parse_timespan(s: &str) -> Result<Duration, TimespanParseError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(TimespanParseError::Empty);
    }

    let mut total_micros: u128 = 0;
    let mut current_number = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            current_number.push(ch);
        } else if ch.is_whitespace() {
            // Whitespace can separate values or be between number and unit
            if !current_number.is_empty() {
                // Check if next is a unit or another number
                if let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        // Continue to unit parsing
                        continue;
                    } else if next.is_ascii_digit() {
                        // Number followed by whitespace and another number means default unit (seconds)
                        let value: u64 = current_number
                            .parse()
                            .map_err(|_| TimespanParseError::InvalidNumber)?;
                        total_micros += value as u128 * 1_000_000;
                        current_number.clear();
                    }
                } else {
                    // Number at end with no unit = seconds
                    let value: u64 = current_number
                        .parse()
                        .map_err(|_| TimespanParseError::InvalidNumber)?;
                    total_micros += value as u128 * 1_000_000;
                    current_number.clear();
                }
            }
        } else if ch.is_ascii_alphabetic() {
            // Parse unit
            let mut unit = String::from(ch);
            while let Some(&next) = chars.peek() {
                if next.is_ascii_alphabetic() {
                    unit.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            if current_number.is_empty() {
                return Err(TimespanParseError::MissingNumber);
            }

            let value: u64 = current_number
                .parse()
                .map_err(|_| TimespanParseError::InvalidNumber)?;

            let micros = match unit.as_str() {
                "us" | "usec" => value as u128,
                "ms" | "msec" => value as u128 * 1_000,
                "s" | "sec" | "second" | "seconds" => value as u128 * 1_000_000,
                "min" | "minute" | "minutes" => value as u128 * 60 * 1_000_000,
                "h" | "hr" | "hour" | "hours" => value as u128 * 60 * 60 * 1_000_000,
                "d" | "day" | "days" => value as u128 * 24 * 60 * 60 * 1_000_000,
                "w" | "week" | "weeks" => value as u128 * 7 * 24 * 60 * 60 * 1_000_000,
                _ => return Err(TimespanParseError::InvalidUnit(unit)),
            };

            total_micros += micros;
            current_number.clear();
        } else {
            return Err(TimespanParseError::InvalidCharacter(ch));
        }
    }

    // Handle remaining number (no unit = seconds)
    if !current_number.is_empty() {
        let value: u64 = current_number
            .parse()
            .map_err(|_| TimespanParseError::InvalidNumber)?;
        total_micros += value as u128 * 1_000_000;
    }

    if total_micros == 0 {
        return Err(TimespanParseError::Empty);
    }

    // Convert microseconds to Duration
    let secs = (total_micros / 1_000_000) as u64;
    let nanos = ((total_micros % 1_000_000) * 1_000) as u32;

    Ok(Duration::new(secs, nanos))
}

/// Error type for timespan parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimespanParseError {
    /// The input string is empty
    Empty,
    /// Invalid number format
    InvalidNumber,
    /// Number without a unit
    MissingNumber,
    /// Unknown time unit
    InvalidUnit(String),
    /// Invalid character in input
    InvalidCharacter(char),
}

impl std::fmt::Display for TimespanParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimespanParseError::Empty => write!(f, "empty timespan"),
            TimespanParseError::InvalidNumber => write!(f, "invalid number format"),
            TimespanParseError::MissingNumber => write!(f, "unit specified without a number"),
            TimespanParseError::InvalidUnit(unit) => write!(f, "invalid time unit: {}", unit),
            TimespanParseError::InvalidCharacter(ch) => {
                write!(f, "invalid character: {}", ch)
            }
        }
    }
}

impl std::error::Error for TimespanParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain_number() {
        assert_eq!(parse_timespan("30"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_timespan("0"), Err(TimespanParseError::Empty));
        assert_eq!(parse_timespan("120"), Ok(Duration::from_secs(120)));
    }

    #[test]
    fn test_parse_seconds() {
        assert_eq!(parse_timespan("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_timespan("1sec"), Ok(Duration::from_secs(1)));
        assert_eq!(parse_timespan("5seconds"), Ok(Duration::from_secs(5)));
    }

    #[test]
    fn test_parse_minutes() {
        assert_eq!(parse_timespan("2min"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_timespan("1minute"), Ok(Duration::from_secs(60)));
        assert_eq!(parse_timespan("5minutes"), Ok(Duration::from_secs(300)));
    }

    #[test]
    fn test_parse_hours() {
        assert_eq!(parse_timespan("1h"), Ok(Duration::from_secs(3600)));
        assert_eq!(parse_timespan("2hr"), Ok(Duration::from_secs(7200)));
        assert_eq!(parse_timespan("1hour"), Ok(Duration::from_secs(3600)));
        assert_eq!(parse_timespan("3hours"), Ok(Duration::from_secs(10800)));
    }

    #[test]
    fn test_parse_days() {
        assert_eq!(parse_timespan("1d"), Ok(Duration::from_secs(86400)));
        assert_eq!(parse_timespan("2days"), Ok(Duration::from_secs(172800)));
    }

    #[test]
    fn test_parse_weeks() {
        assert_eq!(parse_timespan("1w"), Ok(Duration::from_secs(604800)));
        assert_eq!(parse_timespan("2weeks"), Ok(Duration::from_secs(1209600)));
    }

    #[test]
    fn test_parse_milliseconds() {
        assert_eq!(parse_timespan("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_timespan("1000msec"), Ok(Duration::from_millis(1000)));
    }

    #[test]
    fn test_parse_microseconds() {
        assert_eq!(parse_timespan("500us"), Ok(Duration::from_micros(500)));
        assert_eq!(parse_timespan("1000usec"), Ok(Duration::from_micros(1000)));
    }

    #[test]
    fn test_parse_combined() {
        assert_eq!(parse_timespan("2min 30s"), Ok(Duration::from_secs(150)));
        assert_eq!(parse_timespan("1h 30min"), Ok(Duration::from_secs(5400)));
        assert_eq!(
            parse_timespan("1d 2h 3min 4s"),
            Ok(Duration::from_secs(93784))
        );
    }

    #[test]
    fn test_parse_combined_no_space() {
        assert_eq!(parse_timespan("2min30s"), Ok(Duration::from_secs(150)));
        assert_eq!(parse_timespan("1h30min"), Ok(Duration::from_secs(5400)));
    }

    #[test]
    fn test_parse_with_extra_whitespace() {
        assert_eq!(
            parse_timespan("  2min  30s  "),
            Ok(Duration::from_secs(150))
        );
        assert_eq!(parse_timespan("1h    30min"), Ok(Duration::from_secs(5400)));
    }

    #[test]
    fn test_parse_errors() {
        assert_eq!(parse_timespan(""), Err(TimespanParseError::Empty));
        assert_eq!(parse_timespan("   "), Err(TimespanParseError::Empty));
        assert!(parse_timespan("abc").is_err());
        assert!(parse_timespan("10xyz").is_err());
    }

    #[test]
    fn test_parse_subsecond_precision() {
        let result = parse_timespan("1s 500ms").unwrap();
        assert_eq!(result, Duration::from_millis(1500));

        let result = parse_timespan("200ms").unwrap();
        assert_eq!(result.as_millis(), 200);
    }
}
