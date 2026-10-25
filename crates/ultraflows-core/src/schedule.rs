//! Cron schedules (Trigger-shaped, 5-field cron).
//!
//! Trigger uses 5-field cron (minute hour day-of-month month day-of-week,
//! no seconds) with IANA timezones and DST adjustment. UltraFlows uses the
//! `cron` crate for parsing (seconds prepended as `0`), evaluated
//! 2026-10-25 against `croner` 4, which mishandles the spring-forward
//! DST gap (a daily 09:00 America/Havana schedule jumped to 01:00 on
//! the transition day). The `cron` crate passes both the spring-forward
//! and fall-back probes below.
//!
//! Limitation: the `cron` crate rejects Trigger's `L` (last-day-of-month)
//! syntax. Schedules using `L` are rejected with a clear error here,
//! rather than silently misfiring.

use chrono::{DateTime, Utc};
use std::str::FromStr;

/// Next firing time strictly after `from`, in the schedule's timezone,
/// returned as UTC. `pattern` is 5-field Trigger cron.
pub fn next_fire(
    pattern: &str,
    timezone: &str,
    from: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    let tz: chrono_tz::Tz = timezone
        .parse()
        .map_err(|_| format!("unknown timezone: {timezone}"))?;
    let expr = format!("0 {pattern}");
    let schedule = cron::Schedule::from_str(&expr)
        .map_err(|e| format!("invalid cron pattern '{pattern}': {e}"))?;
    schedule
        .after(&from.with_timezone(&tz))
        .next()
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| format!("no next firing time for pattern '{pattern}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_9am_havana_across_spring_forward() {
        let from = DateTime::parse_from_rfc3339("2026-03-07T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // next_fire returns UTC; assert in the schedule's timezone.
        let tz: chrono_tz::Tz = "America/Havana".parse().unwrap();
        let next = next_fire("0 9 * * *", "America/Havana", from).unwrap();
        assert_eq!(
            next.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string(),
            "2026-03-08 09:00"
        );
        let next2 = next_fire("0 9 * * *", "America/Havana", next).unwrap();
        assert_eq!(
            next2
                .with_timezone(&tz)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            "2026-03-09 09:00"
        );
    }

    #[test]
    fn daily_9am_havana_across_fall_back() {
        let from = DateTime::parse_from_rfc3339("2026-10-31T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let tz: chrono_tz::Tz = "America/Havana".parse().unwrap();
        let next = next_fire("0 9 * * *", "America/Havana", from).unwrap();
        assert_eq!(
            next.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string(),
            "2026-11-01 09:00"
        );
    }

    #[test]
    fn every_minute_and_invalid_patterns() {
        let from = DateTime::parse_from_rfc3339("2026-01-01T00:00:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_fire("* * * * *", "UTC", from).unwrap();
        assert_eq!(
            next.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-01-01 00:01:00"
        );
        assert!(next_fire("not a cron", "UTC", from).is_err());
        assert!(next_fire("0 9 * * *", "Mars/Olympus", from).is_err());
        // L (last day) is rejected by the `cron` crate: a clear error,
        // not a silent misfire.
        assert!(next_fire("0 9 L * *", "UTC", from).is_err());
    }
}
