/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Big number types represented as strings.
//!
//! These types are simple string wrappers that allow users to parse and format
//! big numbers using their preferred library.
//!
//! # Accepted input
//!
//! `FromStr` validates the *structure* of the input, not just its character
//! set. The grammars are the JSON number grammar with one deliberate
//! relaxation — leading zeros are accepted:
//!
//! ```text
//! BigInteger := '-'? DIGIT+
//! BigDecimal := '-'? DIGIT+ ( '.' DIGIT+ )? ( ('e' | 'E') ('+' | '-')? DIGIT+ )?
//! ```
//!
//! So `"-12"`, `"00123"`, `"1.23E-10"` parse, while `"+123"`, `".5"`, `"1."`,
//! `"1.2.3"`, `"--5"`, `"1e"`, `"e10"` and `"-"` do not.
//!
//! Leading zeros are accepted because they have exactly one numeric reading and
//! were accepted by previously released versions. They are *not* valid RFC 8259
//! JSON numbers, so a protocol that emits an arbitrary-precision value as a raw
//! JSON number rejects them at the wire boundary instead.

/// Error type for BigInteger and BigDecimal parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigNumberError {
    /// The input string is not a valid number format.
    InvalidFormat(String),
}

impl std::fmt::Display for BigNumberError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BigNumberError::InvalidFormat(s) => write!(f, "invalid number format: {s}"),
        }
    }
}

impl std::error::Error for BigNumberError {}

/// `true` iff `s` is non-empty and consists entirely of ASCII digits.
fn is_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Validates that a string is a valid `BigInteger`: `'-'? DIGIT+`.
///
/// A leading `-` is optional; a leading `+` is not accepted. At least one digit
/// is mandatory, so a bare sign (`"-"`, `"+"`) and the empty string are
/// rejected, as are repeated signs (`"--5"`), decimal points and exponents.
///
/// Leading zeros (`"00123"`) are accepted — see the module docs.
fn is_valid_big_integer(s: &str) -> bool {
    is_ascii_digits(s.strip_prefix('-').unwrap_or(s))
}

/// Validates that a string is a valid `BigDecimal`:
/// `'-'? DIGIT+ ( '.' DIGIT+ )? ( ('e' | 'E') ('+' | '-')? DIGIT+ )?`.
///
/// Each separator that is present must be followed by a non-empty digit run, so
/// `".5"`, `"1."`, `"1e"`, `"1e+"` and `"e10"` are rejected. A repeated
/// separator leaves a non-digit in the following run (`"1.2.3"` leaves
/// `"2.3"`), so it is rejected too. A leading `+` is not accepted.
///
/// Leading zeros (`"00123"`, `"00.1"`) are accepted — see the module docs.
fn is_valid_big_decimal(s: &str) -> bool {
    let rest = s.strip_prefix('-').unwrap_or(s);

    // Split the exponent off first: 'e'/'E' cannot appear in the mantissa.
    let (mantissa, exponent) = match rest.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (rest, None),
    };

    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => (int_part, Some(frac_part)),
        None => (mantissa, None),
    };

    // The integer digit run is mandatory and may not be empty.
    if !is_ascii_digits(int_part) {
        return false;
    }

    // `'.' DIGIT+` — when the point is present the digit run may not be empty.
    if let Some(frac) = frac_part {
        if !is_ascii_digits(frac) {
            return false;
        }
    }

    // `('e' | 'E') ('+' | '-')? DIGIT+` — the sign is optional, the digits are
    // not.
    match exponent {
        None => true,
        Some(exponent) => is_ascii_digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent)),
    }
}

/// A BigInteger represented as a string.
///
/// This type does not perform arithmetic operations. Users should parse the string
/// with their preferred big integer library.
///
/// See the [module docs](self) for the grammar accepted by `FromStr`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    all(aws_sdk_unstable, feature = "serde-deserialize"),
    derive(serde::Deserialize)
)]
#[cfg_attr(
    all(aws_sdk_unstable, feature = "serde-serialize"),
    derive(serde::Serialize)
)]
pub struct BigInteger(String);

impl Default for BigInteger {
    fn default() -> Self {
        Self("0".to_string())
    }
}

impl std::str::FromStr for BigInteger {
    type Err = BigNumberError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !is_valid_big_integer(s) {
            return Err(BigNumberError::InvalidFormat(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }
}

impl AsRef<str> for BigInteger {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A big decimal represented as a string.
///
/// This type does not perform arithmetic operations. Users should parse the string
/// with their preferred big decimal library.
///
/// See the [module docs](self) for the grammar accepted by `FromStr`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    all(aws_sdk_unstable, feature = "serde-deserialize"),
    derive(serde::Deserialize)
)]
#[cfg_attr(
    all(aws_sdk_unstable, feature = "serde-serialize"),
    derive(serde::Serialize)
)]
pub struct BigDecimal(String);

impl Default for BigDecimal {
    fn default() -> Self {
        Self("0.0".to_string())
    }
}

impl std::str::FromStr for BigDecimal {
    type Err = BigNumberError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !is_valid_big_decimal(s) {
            return Err(BigNumberError::InvalidFormat(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }
}

impl AsRef<str> for BigDecimal {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn big_integer_basic() {
        let bi = BigInteger::from_str("12345678901234567890").unwrap();
        assert_eq!(bi.as_ref(), "12345678901234567890");
    }

    #[test]
    fn big_integer_default() {
        let bi = BigInteger::default();
        assert_eq!(bi.as_ref(), "0");
        // The default must itself be parseable.
        assert_eq!(BigInteger::from_str(bi.as_ref()).unwrap(), bi);
    }

    #[test]
    fn big_decimal_basic() {
        let bd = BigDecimal::from_str("123.456789").unwrap();
        assert_eq!(bd.as_ref(), "123.456789");
    }

    #[test]
    fn big_decimal_default() {
        let bd = BigDecimal::default();
        assert_eq!(bd.as_ref(), "0.0");
        assert_eq!(BigDecimal::from_str(bd.as_ref()).unwrap(), bd);
    }

    #[test]
    fn big_integer_negative() {
        let bi = BigInteger::from_str("-12345").unwrap();
        assert_eq!(bi.as_ref(), "-12345");
    }

    #[test]
    fn big_decimal_scientific() {
        let bd = BigDecimal::from_str("1.23e10").unwrap();
        assert_eq!(bd.as_ref(), "1.23e10");

        let bd = BigDecimal::from_str("1.23E-10").unwrap();
        assert_eq!(bd.as_ref(), "1.23E-10");
    }

    #[test]
    fn big_integer_rejects_json_injection() {
        // Reject strings with JSON special characters
        assert!(BigInteger::from_str("123, \"injected\": true").is_err());
        assert!(BigInteger::from_str("123}").is_err());
        assert!(BigInteger::from_str("{\"hacked\": 1}").is_err());
        assert!(BigInteger::from_str("123\"").is_err());
        assert!(BigInteger::from_str("123\\n456").is_err());
    }

    #[test]
    fn big_decimal_rejects_json_injection() {
        assert!(BigDecimal::from_str("123.45, \"injected\": true").is_err());
        assert!(BigDecimal::from_str("123.45}").is_err());
        assert!(BigDecimal::from_str("{\"hacked\": 1.0}").is_err());
    }

    #[test]
    fn big_integer_rejects_invalid_chars() {
        assert!(BigInteger::from_str("abc").is_err());
        assert!(BigInteger::from_str("123abc").is_err());
        assert!(BigInteger::from_str("12 34").is_err());
        assert!(BigInteger::from_str("").is_err());
    }

    #[test]
    fn big_integer_rejects_decimal_and_scientific() {
        // BigInteger should reject decimal points
        assert!(BigInteger::from_str("123.45").is_err());
        assert!(BigInteger::from_str("123.0").is_err());

        // BigInteger should reject scientific notation
        assert!(BigInteger::from_str("1e10").is_err());
        assert!(BigInteger::from_str("1E10").is_err());
        assert!(BigInteger::from_str("1.23e10").is_err());
    }

    #[test]
    fn big_decimal_rejects_invalid_chars() {
        assert!(BigDecimal::from_str("abc").is_err());
        assert!(BigDecimal::from_str("123.45abc").is_err());
        assert!(BigDecimal::from_str("12.34 56").is_err());
        assert!(BigDecimal::from_str("").is_err());
    }

    // --- Structural grammar: only a leading '-' is a sign -----------------------

    #[test]
    fn big_numbers_reject_a_leading_plus() {
        // `+123` is not a JSON number and has no single canonical
        // representation, so it is not accepted (a behavior change from
        // 1.6.4, which validated only the character set).
        assert!(BigInteger::from_str("+123").is_err());
        assert!(BigInteger::from_str("+0").is_err());
        assert!(BigDecimal::from_str("+1.0").is_err());
        assert!(BigDecimal::from_str("+1e3").is_err());
    }

    #[test]
    fn big_numbers_reject_bare_and_repeated_signs() {
        for bad in ["-", "+", "--5", "++5", "-+5", "+-5", "5-", "5+"] {
            assert!(
                BigInteger::from_str(bad).is_err(),
                "BigInteger::from_str({bad:?}) should fail"
            );
            assert!(
                BigDecimal::from_str(bad).is_err(),
                "BigDecimal::from_str({bad:?}) should fail"
            );
        }
    }

    #[test]
    fn big_decimal_requires_digits_around_every_separator() {
        for bad in [
            ".5",     // no integer digits
            "-.5",    // no integer digits after the sign
            "1.",     // no fractional digits
            "1..2",   // repeated point
            "1.2.3",  // repeated point
            "0.0.0",  // repeated point
            "1e",     // no exponent digits
            "1E",     // no exponent digits
            "1e+",    // sign but no exponent digits
            "1e-",    // sign but no exponent digits
            "e10",    // no mantissa
            "-e10",   // no mantissa after the sign
            "1e2e3",  // repeated exponent
            "1.5e",   // no exponent digits
            "1e2.5",  // fractional exponent
            "1e 2",   // space inside the exponent
            "1 .5",   // space inside the mantissa
            ".",      // point only
            "1.2e3e", // trailing exponent marker
        ] {
            assert!(
                BigDecimal::from_str(bad).is_err(),
                "BigDecimal::from_str({bad:?}) should fail"
            );
        }
    }

    #[test]
    fn big_decimal_accepts_the_full_json_number_grammar() {
        for good in [
            "0",
            "-0",
            "123",
            "-123",
            "1.5",
            "-0.5",
            "1e3",
            "1E3",
            "1e+9",
            "1e-9",
            "1.23e10",
            "1.23E-10",
            "10.0",
            "5e-3",
            "0.123456789012345678901234567890",
            "12345678901234567890.123",
            "1.234e500",
        ] {
            let bd = BigDecimal::from_str(good)
                .unwrap_or_else(|e| panic!("BigDecimal::from_str({good:?}) should succeed: {e}"));
            // The stored text is exactly the input: parsing never rewrites it.
            assert_eq!(bd.as_ref(), good);
        }
    }

    #[test]
    fn big_numbers_still_accept_leading_zeros() {
        // Retained for compatibility with previously released versions:
        // leading zeros have one numeric reading. They are *not* valid RFC
        // 8259 JSON numbers, and are rejected separately when an
        // arbitrary-precision value is emitted as a raw JSON number.
        for good in ["00123", "007", "-01", "0000", "-0000"] {
            let bi = BigInteger::from_str(good)
                .unwrap_or_else(|e| panic!("BigInteger::from_str({good:?}) should succeed: {e}"));
            assert_eq!(bi.as_ref(), good);
            let bd = BigDecimal::from_str(good)
                .unwrap_or_else(|e| panic!("BigDecimal::from_str({good:?}) should succeed: {e}"));
            assert_eq!(bd.as_ref(), good);
        }
        for good in ["00.1", "-01.5", "007e2"] {
            let bd = BigDecimal::from_str(good)
                .unwrap_or_else(|e| panic!("BigDecimal::from_str({good:?}) should succeed: {e}"));
            assert_eq!(bd.as_ref(), good);
        }
    }

    #[test]
    fn big_integer_accepts_only_digits_after_an_optional_minus() {
        for good in ["0", "-0", "1", "-1", "12345678901234567890"] {
            assert!(
                BigInteger::from_str(good).is_ok(),
                "BigInteger::from_str({good:?}) should succeed"
            );
        }
        for bad in ["1_000", "1,000", "0x1f", " 1", "1 ", "\t1", "1\n"] {
            assert!(
                BigInteger::from_str(bad).is_err(),
                "BigInteger::from_str({bad:?}) should fail"
            );
        }
    }

    #[test]
    fn big_number_error_reports_the_offending_input() {
        let err = BigInteger::from_str("+123").unwrap_err();
        assert_eq!(err, BigNumberError::InvalidFormat("+123".to_string()));
        assert!(err.to_string().contains("+123"));
    }
}
