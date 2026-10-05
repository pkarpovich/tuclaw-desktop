use std::sync::LazyLock;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use time::macros::format_description;
use time::{Date, OffsetDateTime, UtcOffset};

static ZONE: LazyLock<TimeZone> = LazyLock::new(TimeZone::system);

pub fn local(at: OffsetDateTime) -> OffsetDateTime {
    in_zone(at, &ZONE)
}

pub fn today() -> Date {
    local(OffsetDateTime::now_utc()).date()
}

pub fn clock(at: OffsetDateTime) -> String {
    let description = format_description!("[hour repr:12 padding:none]:[minute] [period]");
    local(at).format(&description).unwrap_or_default()
}

pub fn when(at: OffsetDateTime, now: OffsetDateTime) -> String {
    let day = local(at).date();
    let today = local(now).date();
    let time = clock(at);
    let distance = (day - today).whole_days();
    match distance {
        0 => time,
        1 => format!("tomorrow {time}"),
        -1 => format!("yesterday {time}"),
        -6..=6 => {
            let description = format_description!("[weekday repr:short]");
            format!(
                "{} {time}",
                local(at).format(&description).unwrap_or_default()
            )
        }
        _ => {
            let description = format_description!("[month repr:short] [day padding:none]");
            format!(
                "{} {time}",
                local(at).format(&description).unwrap_or_default()
            )
        }
    }
}

fn in_zone(at: OffsetDateTime, zone: &TimeZone) -> OffsetDateTime {
    let Ok(instant) = Timestamp::from_second(at.unix_timestamp()) else {
        return at;
    };
    let seconds = zone.to_offset(instant).seconds();
    let Ok(offset) = UtcOffset::from_whole_seconds(seconds) else {
        return at;
    };
    at.to_offset(offset)
}

#[cfg(test)]
mod tests {
    use jiff::tz::TimeZone;
    use time::macros::{datetime, offset};

    use super::{in_zone, when};

    #[test]
    fn warsaw_follows_its_daylight_saving_time() {
        let warsaw = TimeZone::get("Europe/Warsaw").expect("the tz database has Warsaw");
        let summer = in_zone(datetime!(2026-10-04 19:54 UTC), &warsaw);
        assert_eq!(summer.offset(), offset!(+2));
        assert_eq!((summer.hour(), summer.minute()), (21, 54));
        let winter = in_zone(datetime!(2026-12-04 19:54 UTC), &warsaw);
        assert_eq!(winter.offset(), offset!(+1));
        assert_eq!(winter.hour(), 20);
        assert_eq!(
            in_zone(datetime!(2026-10-04 22:30 UTC), &warsaw).date(),
            datetime!(2026-10-05 00:00 UTC).date()
        );
    }

    #[test]
    fn a_time_names_its_day_relative_to_now() {
        let now = datetime!(2026-10-05 12:00 UTC);
        let clock = super::clock;
        let same = datetime!(2026-10-05 13:00 UTC);
        assert_eq!(when(same, now), clock(same));
        let next = datetime!(2026-10-06 12:00 UTC);
        assert_eq!(when(next, now), format!("tomorrow {}", clock(next)));
        let wednesday = datetime!(2026-10-07 12:00 UTC);
        assert_eq!(when(wednesday, now), format!("Wed {}", clock(wednesday)));
        let far = datetime!(2026-10-19 12:00 UTC);
        assert_eq!(when(far, now), format!("Oct 19 {}", clock(far)));
    }

    #[test]
    fn utc_leaves_the_time_as_it_is() {
        let at = datetime!(2026-10-04 19:54 UTC);
        assert_eq!(in_zone(at, &TimeZone::UTC), at);
    }
}
