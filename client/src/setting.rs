//! A setting's value as a person types it, parsed once at the edge into what the protocol
//! carries: the CLI's `phantom settings set` and the app's Settings fields both go through here,
//! so they refuse the same text for the same reason, in [`display::value_problem`]'s words.
//!
//! [`display::value_problem`]: crate::display::value_problem

use std::num::NonZeroU16;

/// Why typed text can't be a setting's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueProblem {
    /// Not a whole number.
    NotAPort,
    /// A number, but 0 or above 65535.
    PortOutOfRange,
    NotOnOrOff,
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

        #[test]
        fn nothing_or_only_spaces_is_not_a_port(text in spaces()) {
            prop_assert_eq!(port(&text), Err(ValueProblem::NotAPort));
        }
    }
}
