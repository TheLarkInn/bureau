//! UTC datetime validation for preserved SDK workspace metadata.

fn number(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

const fn leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

const fn days(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn date_parts(value: &str) -> Option<(u32, u32, u32)> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    Some((
        value.get(..4).and_then(number)?,
        value.get(5..7).and_then(number)?,
        value.get(8..).and_then(number)?,
    ))
}

fn date(value: &str) -> bool {
    let Some((year, month, day)) = date_parts(value) else {
        return false;
    };
    (1..=days(year, month)).contains(&day)
}

fn component(value: &str, maximum: u32) -> bool {
    value.len() == 2 && number(value).is_some_and(|value| value <= maximum)
}

fn seconds(value: &str) -> bool {
    let Some(value) = value.strip_prefix(':') else {
        return value.is_empty();
    };
    let (seconds, fraction) = value
        .split_once('.')
        .map_or((value, None), |(seconds, fraction)| {
            (seconds, Some(fraction))
        });
    component(seconds, 59)
        && fraction.is_none_or(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn time_parts(value: &str) -> Option<(&str, &str, &str)> {
    if value.as_bytes().get(2) != Some(&b':') {
        return None;
    }
    Some((value.get(..2)?, value.get(3..5)?, value.get(5..)?))
}

fn clock(value: &str) -> bool {
    let Some((hour, minute, remaining)) = time_parts(value) else {
        return false;
    };
    component(hour, 23) && component(minute, 59) && seconds(remaining)
}

pub(super) fn valid(value: &str) -> bool {
    let Some(value) = value.strip_suffix('Z') else {
        return false;
    };
    let Some((day, time)) = value.split_once('T') else {
        return false;
    };
    date(day) && clock(time)
}
