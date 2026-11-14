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

/// A speed limit: a number (with a fraction or not) and a unit, `B`, `K`/`KB`/`KiB`, `M`/`MB`/`MiB`
/// or `G`/`GB`/`GiB`, in any case, with or without `/s` and spaces; without a unit, KiB. Every unit
/// is binary, as Phantom shows speeds. Rounded to whole bytes.
pub fn speed(text: &str) -> Result<SpeedLimit, ValueProblem> {
    let text = text.trim().to_ascii_lowercase();
    let text = text.strip_suffix("/s").unwrap_or(&text).trim_end();
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let unit: u64 = match unit.trim() {
        "b" => 1,
        "" | "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        _ => return Err(ValueProblem::NotASpeed),
    };
    if !number.bytes().any(|b| b.is_ascii_digit()) {
        return Err(ValueProblem::NotASpeed);
    }
    let value: f64 = number.parse().map_err(|_| ValueProblem::NotASpeed)?;
    let bytes = (value * unit as f64).round();
    // Past any limit: no u64 needed to say so.
    let bytes = if bytes > SpeedLimit::MAX as f64 { u64::MAX } else { bytes as u64 };
    SpeedLimit::new(bytes).map_err(ValueProblem::Limit)
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

        #[test]
        fn a_fraction_rounds_to_whole_bytes(whole in 16..1000u64, tenths in 0..10u64) {
            let bytes = ((whole as f64 + tenths as f64 / 10.0) * 1024.0).round() as u64;
            prop_assert_eq!(speed(&format!("{whole}.{tenths} KiB/s")).map(|l| u64::from(l)), Ok(bytes));
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
