//! Deterministic wall-clock formatting for terminal timestamps.

/// Milliseconds in one minute.
pub const MS_PER_MINUTE: i64 = 60_000;
/// Milliseconds in one hour.
pub const MS_PER_HOUR: i64 = 3_600_000;
/// Milliseconds in one UTC day.
pub const MS_PER_DAY: i64 = 86_400_000;
/// Milliseconds in seven UTC days.
pub const MS_PER_WEEK: i64 = 604_800_000;

/// Calendar grouping for a session relative to wall-clock time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgeBucket {
    /// The same UTC day as `wall_ms`.
    Today,
    /// The UTC day immediately before `wall_ms`.
    Yesterday,
    /// Two through six UTC days before `wall_ms`.
    ThisWeek,
    /// Seven or more UTC days before `wall_ms`.
    Older,
}

/// Compact relative age derived from two Unix-epoch millisecond instants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeAge {
    /// Less than one minute, including a future timestamp.
    JustNow,
    /// Whole minutes in `1..=59`.
    Minutes(u64),
    /// Whole hours in `1..=23`.
    Hours(u64),
    /// Whole days in `1..=6`.
    Days(u64),
    /// Whole weeks of seven UTC days.
    Weeks(u64),
}

/// Returns the compact relative age of `updated_at_ms` at `wall_ms`.
#[must_use]
pub fn relative_age(updated_at_ms: i64, wall_ms: i64) -> RelativeAge {
    let delta = wall_ms.saturating_sub(updated_at_ms).max(0);
    if delta < MS_PER_MINUTE {
        RelativeAge::JustNow
    } else if delta < MS_PER_HOUR {
        RelativeAge::Minutes(positive_quot(delta, MS_PER_MINUTE))
    } else if delta < MS_PER_DAY {
        RelativeAge::Hours(positive_quot(delta, MS_PER_HOUR))
    } else if delta < MS_PER_WEEK {
        RelativeAge::Days(positive_quot(delta, MS_PER_DAY))
    } else {
        RelativeAge::Weeks(positive_quot(delta, MS_PER_WEEK))
    }
}

/// Formats a compact relative age for session metadata.
#[must_use]
pub fn format_relative_age(age: RelativeAge) -> String {
    match age {
        RelativeAge::JustNow => "just now".to_owned(),
        RelativeAge::Minutes(count) => format!("{count}m ago"),
        RelativeAge::Hours(count) => format!("{count}h ago"),
        RelativeAge::Days(count) => format!("{count}d ago"),
        RelativeAge::Weeks(count) => format!("{count}w ago"),
    }
}

/// Returns the UTC calendar bucket of `updated_at_ms` at `wall_ms`.
#[must_use]
pub fn age_bucket(updated_at_ms: i64, wall_ms: i64) -> AgeBucket {
    let updated_day = updated_at_ms.div_euclid(MS_PER_DAY);
    let wall_day = wall_ms.div_euclid(MS_PER_DAY);
    match wall_day.saturating_sub(updated_day) {
        delta if delta <= 0 => AgeBucket::Today,
        1 => AgeBucket::Yesterday,
        2..=6 => AgeBucket::ThisWeek,
        _ => AgeBucket::Older,
    }
}

fn positive_quot(delta: i64, unit: i64) -> u64 {
    u64::try_from(delta / unit).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        AgeBucket, MS_PER_DAY, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_WEEK, RelativeAge, age_bucket,
        format_relative_age, relative_age,
    };

    const NOON: i64 = 20_000 * MS_PER_DAY + 12 * MS_PER_HOUR;

    #[test]
    fn relative_age_is_a_pure_function_of_two_integers() {
        assert_eq!(relative_age(NOON, NOON), RelativeAge::JustNow);
        assert_eq!(
            relative_age(NOON, NOON + MS_PER_MINUTE - 1),
            RelativeAge::JustNow
        );
        assert_eq!(
            relative_age(NOON, NOON + MS_PER_MINUTE),
            RelativeAge::Minutes(1)
        );
        assert_eq!(
            relative_age(NOON, NOON + 59 * MS_PER_MINUTE),
            RelativeAge::Minutes(59)
        );
        assert_eq!(
            relative_age(NOON, NOON + MS_PER_HOUR),
            RelativeAge::Hours(1)
        );
        assert_eq!(
            relative_age(NOON, NOON + 23 * MS_PER_HOUR),
            RelativeAge::Hours(23)
        );
        assert_eq!(relative_age(NOON, NOON + MS_PER_DAY), RelativeAge::Days(1));
        assert_eq!(
            relative_age(NOON, NOON + 6 * MS_PER_DAY),
            RelativeAge::Days(6)
        );
        assert_eq!(
            relative_age(NOON, NOON + MS_PER_WEEK),
            RelativeAge::Weeks(1)
        );
        assert_eq!(relative_age(NOON + 1, NOON), RelativeAge::JustNow);
    }

    #[test]
    fn relative_age_labels_are_stable() {
        assert_eq!(format_relative_age(RelativeAge::JustNow), "just now");
        assert_eq!(format_relative_age(RelativeAge::Minutes(2)), "2m ago");
        assert_eq!(format_relative_age(RelativeAge::Hours(5)), "5h ago");
        assert_eq!(format_relative_age(RelativeAge::Days(3)), "3d ago");
        assert_eq!(format_relative_age(RelativeAge::Weeks(4)), "4w ago");
    }

    #[test]
    fn age_buckets_respect_utc_day_and_week_boundaries() {
        let day_start = 20_000 * MS_PER_DAY;
        let just_before_midnight = day_start - 1;
        let next_morning = day_start + 60;
        assert_eq!(
            age_bucket(just_before_midnight, next_morning),
            AgeBucket::Yesterday
        );
        assert_eq!(
            age_bucket(day_start, day_start + MS_PER_DAY - 1),
            AgeBucket::Today
        );
        assert_eq!(
            age_bucket(day_start, day_start + 2 * MS_PER_DAY),
            AgeBucket::ThisWeek
        );
        assert_eq!(
            age_bucket(day_start, day_start + 6 * MS_PER_DAY),
            AgeBucket::ThisWeek
        );
        assert_eq!(
            age_bucket(day_start, day_start + 7 * MS_PER_DAY),
            AgeBucket::Older
        );
        assert_eq!(age_bucket(day_start + 1, day_start), AgeBucket::Today);
    }
}
