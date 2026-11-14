//! A setting's value as a person types it, parsed once at the edge into what the protocol
//! carries: the CLI's `phantom settings set` and the app's Settings fields both go through here,
//! so they refuse the same text for the same reason, in [`display::value_problem`]'s words.
//!
//! [`display::value_problem`]: crate::display::value_problem

use std::num::NonZeroU16;

use proto::{LimitProblem, SpeedLimit};

/// Why typed text can't be a setting's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueProblem {
    /// Not a whole number.
    NotAPort,
    /// A number, but 0 or above 65535.
    PortOutOfRange,
    NotOnOrOff,
    /// Not a number with a unit.
    NotASpeed,
    /// A speed, but not one a limit can be.
    Limit(LimitProblem),
}

/// A speed limit as typed: a number, a unit, and optionally `/s`, in any case, with or without
/// spaces around the number and the unit. Every unit is binary, as Phantom shows speeds, and a
/// number without a unit is in KiB. The number is whole or has a fraction of up to three digits
/// (`1.`, `.5` and `1.5` are all numbers); the result is rounded half up to whole bytes, in
/// integers.
///
/// | Typed | Means |
/// |---|---|
/// | `500` | 500 KiB/s |
/// | `750k` | 750 KiB/s |
/// | `1.5M` | 1.5 MiB/s, 1572864 B/s |
/// | `2 MiB/s` | 2 MiB/s |
/// | `20000 b` | 20000 B/s |
///
/// | Typed | Refused as |
/// |---|---|
/// | `fast`, `5 x`, `5 MB/h` (no number, or an unknown unit) | [`ValueProblem::NotASpeed`] |
/// | `.`, `1.2.3`, `1.2345 M` (not a number, or a fraction of more than three digits) | `NotASpeed` |
/// | `0`, `15 KiB` (below [`SpeedLimit::MIN`]) | [`ValueProblem::Limit`]`(TooLow)` |
/// | `4 GiB`, `99999999999 G` (above [`SpeedLimit::MAX`]) | `Limit(TooHigh)` |
pub fn speed(text: &str) -> Result<SpeedLimit, ValueProblem> {
    let text = text.trim().to_ascii_lowercase();
    let (number, unit) = split_number_and_unit(without_per_second(&text));
    let size = unit_size(unit)?;
    let thousandths = thousandths(number)?;
    let bytes = (thousandths * u128::from(size) + 500) / 1000;
    // Past any limit: no more than u64 is needed to say so.
    SpeedLimit::new(u64::try_from(bytes).unwrap_or(u64::MAX)).map_err(ValueProblem::Limit)
}

/// Every spelling of a unit and its size in bytes: no unit is KiB.
const UNITS: [(&str, u64); 11] = [
    ("b", 1),
    ("", 1 << 10),
    ("k", 1 << 10),
    ("kb", 1 << 10),
    ("kib", 1 << 10),
    ("m", 1 << 20),
    ("mb", 1 << 20),
    ("mib", 1 << 20),
    ("g", 1 << 30),
    ("gb", 1 << 30),
    ("gib", 1 << 30),
];

/// `text` without a closing `/s` and the spaces before it: `2 mib /s` is `2 mib`.
fn without_per_second(text: &str) -> &str {
    text.strip_suffix("/s").unwrap_or(text).trim_end()
}

/// The digits and points at the start, and the unit after them (spaces around it dropped).
fn split_number_and_unit(text: &str) -> (&str, &str) {
    let end = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (number, unit) = text.split_at(end);
    (number, unit.trim())
}

/// How many bytes a unit is; an unknown one isn't a speed.
fn unit_size(unit: &str) -> Result<u64, ValueProblem> {
    UNITS.iter().find(|(spelling, _)| *spelling == unit).map(|&(_, size)| size).ok_or(ValueProblem::NotASpeed)
}

/// A number in thousandths: `1.5` is 1500. Needs a digit and at most three after the point; a
/// whole part too big for a u64 counts as the biggest, which no limit is.
fn thousandths(number: &str) -> Result<u128, ValueProblem> {
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let digits = |text: &str| text.bytes().all(|b| b.is_ascii_digit());
    if whole.len() + fraction.len() == 0 || fraction.len() > 3 || !digits(whole) || !digits(fraction) {
        return Err(ValueProblem::NotASpeed);
    }
    let whole = if whole.is_empty() { 0 } else { whole.parse::<u64>().unwrap_or(u64::MAX) };
    let fraction = if fraction.is_empty() { 0 } else { format!("{fraction:0<3}").parse::<u64>().expect("three digits") };
    Ok(u128::from(whole) * 1000 + u128::from(fraction))
}

/// A listening port: a whole number from 1 to 65535, spaces around it ignored.
pub fn port(text: &str) -> Result<NonZeroU16, ValueProblem> {
    let digits = text.trim();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ValueProblem::NotAPort);
    }
    digits.parse::<u16>().ok().and_then(NonZeroU16::new).ok_or(ValueProblem::PortOutOfRange)
}

/// A switch: `on` or `off`, in any case.
pub fn on_off(text: &str) -> Result<bool, ValueProblem> {
    match text.trim().to_ascii_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(ValueProblem::NotOnOrOff),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn spaces() -> impl Strategy<Value = String> {
        "[ \t]{0,3}"
    }

    proptest! {
        #[test]
        fn every_port_reads_back_with_spaces_and_leading_zeros(port in 1..=u16::MAX, zeros in "0{0,3}", before in spaces(), after in spaces()) {
            prop_assert_eq!(super::port(&format!("{before}{zeros}{port}{after}")).map(NonZeroU16::get), Ok(port));
        }

        #[test]
        fn a_number_outside_1_to_65535_is_out_of_range(n in prop_oneof![Just(0u64), 65536..u64::MAX]) {
            prop_assert_eq!(port(&n.to_string()), Err(ValueProblem::PortOutOfRange));
        }

        #[test]
        fn text_with_anything_but_digits_is_not_a_port(before in "[0-9]{0,3}", odd in "[^0-9\\s]", after in "[0-9]{0,3}") {
            prop_assert_eq!(port(&format!("{before}{odd}{after}")), Err(ValueProblem::NotAPort));
        }

        #[test]
        fn on_and_off_in_any_case_and_nothing_else(text in "[a-zA-Z]{0,4}", before in spaces()) {
            let expected = match text.to_ascii_lowercase().as_str() {
                "on" => Ok(true),
                "off" => Ok(false),
                _ => Err(ValueProblem::NotOnOrOff),
            };
            prop_assert_eq!(on_off(&format!("{before}{text}")), expected);
        }

        /// What the app shows for a limit reads back as that limit.
        #[test]
        fn a_limit_shown_reads_back(bytes in SpeedLimit::MIN..=SpeedLimit::MAX, before in spaces(), after in spaces()) {
            let limit = SpeedLimit::new(bytes).unwrap();
            prop_assert_eq!(speed(&format!("{before}{}{after}", crate::display::speed_limit(limit))), Ok(limit));
        }

        /// A whole number in each unit, every way of writing it, is that many bytes a second, or
        /// refused as too low or too high.
        #[test]
        fn a_number_and_a_unit_is_that_many_bytes(
            n in 0..5_000_000u64,
            (unit, size) in proptest::sample::select(vec![("b", 1u64), ("k", 1 << 10), ("m", 1 << 20), ("g", 1 << 30), ("", 1 << 10)]),
            spelling in 0..3usize,
            per_second in any::<bool>(),
            space in any::<bool>(),
            upper in any::<bool>(),
        ) {
            let unit = match (unit, spelling) {
                ("b" | "", _) | (_, 0) => unit.to_owned(),
                (u, 1) => format!("{u}b"),
                (u, _) => format!("{u}ib"),
            };
            let text = format!("{n}{}{unit}{}", if space { " " } else { "" }, if per_second && !unit.is_empty() { "/s" } else { "" });
            let text = if upper { text.to_ascii_uppercase() } else { text };
            prop_assert_eq!(speed(&text), SpeedLimit::new(n.saturating_mul(size)).map_err(ValueProblem::Limit), "{}", text);
        }

        /// Any spelling of a unit, with any spaces and case, is the same value: here a number with
        /// a fraction, in a spelling of KiB, against `number KiB`.
        #[test]
        fn every_spelling_spacing_and_case_is_the_same_value(
            whole in 16..5000u64,
            fraction in "[0-9]{0,3}",
            spelling in proptest::sample::select(vec!["", "k", "kb", "kib"]),
            (before, between, inside, after) in (spaces(), spaces(), spaces(), spaces()),
            per_second in any::<bool>(),
            upper in any::<bool>(),
        ) {
            let number = if fraction.is_empty() { whole.to_string() } else { format!("{whole}.{fraction}") };
            let per_second = if per_second && !spelling.is_empty() { format!("{inside}/s") } else { String::new() };
            let text = format!("{before}{number}{between}{spelling}{per_second}{after}");
            let text = if upper { text.to_ascii_uppercase() } else { text };
            prop_assert_eq!(speed(&text), speed(&format!("{number} KiB")), "{:?}", text);
        }

        /// A fraction of up to three digits is exact in integers, rounded half up to whole bytes.
        #[test]
        fn a_fraction_rounds_half_up_to_whole_bytes(whole in 16..1000u64, tenths in 0..10u64) {
            let bytes = ((whole * 10 + tenths) * 1024 + 5) / 10;
            prop_assert_eq!(speed(&format!("{whole}.{tenths} KiB/s")).map(u64::from), Ok(bytes));
        }

        #[test]
        fn half_a_byte_rounds_up(n in SpeedLimit::MIN..SpeedLimit::MAX) {
            prop_assert_eq!(speed(&format!("{n}.5 B")).map(u64::from), Ok(n + 1));
            prop_assert_eq!(speed(&format!("{n}.499 B")).map(u64::from), Ok(n));
        }

        /// A point with nothing before or after it is fine as long as there's a digit.
        #[test]
        fn a_bare_point_beside_digits_is_a_number(n in 16..5000u64, tenth in 0..10u64) {
            prop_assert_eq!(speed(&format!("{n}. KiB")), speed(&format!("{n} KiB")));
            prop_assert_eq!(speed(&format!(".{tenth} MiB")), speed(&format!("0.{tenth} MiB")));
        }

        #[test]
        fn more_than_three_fraction_digits_is_not_a_speed(whole in 16..5000u64, fraction in "[0-9]{4,6}") {
            prop_assert_eq!(speed(&format!("{whole}.{fraction} KiB")), Err(ValueProblem::NotASpeed));
        }

        /// Whatever the unit isn't one of, with a number before it, isn't a speed.
        #[test]
        fn an_unknown_unit_is_not_a_speed(
            number in "[0-9]{1,3}",
            unit in "[a-zA-Z]{1,5}",
            space in spaces(),
            per_second in any::<bool>(),
        ) {
            let known = ["b", "k", "kb", "kib", "m", "mb", "mib", "g", "gb", "gib"];
            prop_assume!(!known.contains(&unit.to_ascii_lowercase().as_str()));
            let per_second = if per_second { "/s" } else { "" };
            prop_assert_eq!(speed(&format!("{number}{space}{unit}{per_second}")), Err(ValueProblem::NotASpeed));
        }

        #[test]
        fn text_that_isnt_a_number_and_unit_is_not_a_speed(text in prop_oneof!["[a-z ]{0,6}", "[0-9]{1,3}[a-z&&[^bkmgi]]{1,3}", "[0-9]{1,3}\\.[0-9]\\.[0-9]", "\\.[ kmg]?"]) {
            prop_assert_eq!(speed(&text), Err(ValueProblem::NotASpeed), "{}", text);
        }

        #[test]
        fn nothing_or_only_spaces_is_not_a_port(text in spaces()) {
            prop_assert_eq!(port(&text), Err(ValueProblem::NotAPort));
        }
    }
}
