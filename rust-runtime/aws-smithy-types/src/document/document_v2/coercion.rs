/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Numeric coercion for [`DocumentV2`].
//!
//! The SEP's number coercion rules say that numeric accessors convert from any numeric type,
//! that loss of precision is ignored, and that overflow is handled idiomatically. Here that
//! means a fractional value is truncated toward zero when an integer is requested, and a value
//! outside the requested type's range is an error rather than a wrapped or saturated value.

use super::{variant_name, DocumentV2};
use crate::{BigDecimal, BigInteger, DocumentError, Number};
use std::str::FromStr;

/// The most integer digits [`truncate_decimal`] produces. A decimal exponent can describe an
/// integer with billions of digits (`1e1000000000`); this limit stops such a value from causing
/// a huge allocation. About a million digits is far beyond any practical value.
const MAX_INTEGER_DIGITS: usize = 1 << 20;

/// Converts a numeric document to a signed integer type no wider than `i64`.
pub(super) fn to_signed<T>(document: &DocumentV2, target: &'static str) -> Result<T, DocumentError>
where
    T: TryFrom<i128>,
{
    let value: i128 = match document {
        DocumentV2::Number(Number::PosInt(v)) => i128::from(*v),
        DocumentV2::Number(Number::NegInt(v)) => i128::from(*v),
        DocumentV2::Number(Number::Float(v)) => {
            if !v.is_finite() {
                return Err(non_finite(*v, target));
            }
            // `as` saturates, and a saturated value is outside every target's range.
            v.trunc() as i128
        }
        DocumentV2::BigInteger(v) => {
            // The `BigInteger` grammar guarantees digits, so parsing only fails on overflow.
            i128::from_str(v.as_ref()).map_err(|_| overflow(target, document))?
        }
        DocumentV2::BigDecimal(v) => {
            i128::from_str(&truncate_decimal(v, target)?).map_err(|_| overflow(target, document))?
        }
        other => return Err(type_mismatch(target, other)),
    };
    T::try_from(value).map_err(|_| overflow(target, document))
}

/// Converts a numeric document to `f64`.
pub(super) fn to_f64(document: &DocumentV2) -> Result<f64, DocumentError> {
    to_f64_for(document, "double")
}

/// Converts a numeric document to `f32`.
pub(super) fn to_f32(document: &DocumentV2) -> Result<f32, DocumentError> {
    let wide = to_f64_for(document, "float")?;
    let narrow = wide as f32;
    if wide.is_finite() && narrow.is_infinite() {
        return Err(overflow("float", document));
    }
    Ok(narrow)
}

/// Converts a numeric document to `f64`, naming `target` in errors.
fn to_f64_for(document: &DocumentV2, target: &'static str) -> Result<f64, DocumentError> {
    let text = match document {
        DocumentV2::Number(number) => return Ok(number.to_f64_lossy()),
        DocumentV2::BigInteger(v) => v.as_ref(),
        DocumentV2::BigDecimal(v) => v.as_ref(),
        other => return Err(type_mismatch(target, other)),
    };
    // Every `BigInteger` and `BigDecimal` string is valid `f64` syntax. Parsing rounds to the
    // nearest `f64`, and a value too large for `f64` parses as infinity.
    let value = f64::from_str(text).map_err(|e| {
        DocumentError::invalid_input(format!("cannot parse {text:?} as {target}: {e}"))
    })?;
    if value.is_infinite() {
        return Err(overflow(target, document));
    }
    Ok(value)
}

/// Converts a numeric document to a [`BigInteger`].
pub(super) fn to_big_integer(document: &DocumentV2) -> Result<BigInteger, DocumentError> {
    let text = match document {
        DocumentV2::BigInteger(v) => return Ok(v.clone()),
        DocumentV2::Number(Number::PosInt(v)) => v.to_string(),
        DocumentV2::Number(Number::NegInt(v)) => v.to_string(),
        DocumentV2::Number(Number::Float(v)) => {
            if !v.is_finite() {
                return Err(non_finite(*v, "bigInteger"));
            }
            // `{:.0}` prints every integer digit of an `f64`, without an exponent. Adding
            // `0.0` turns `-0.0` into `0.0`, which prints as "0" rather than "-0".
            format!("{:.0}", v.trunc() + 0.0)
        }
        DocumentV2::BigDecimal(v) => truncate_decimal(v, "bigInteger")?,
        other => return Err(type_mismatch("bigInteger", other)),
    };
    BigInteger::from_str(&text).map_err(|e| {
        DocumentError::invalid_input(format!("cannot parse {text:?} as bigInteger: {e}"))
    })
}

/// Converts a numeric document to a [`BigDecimal`].
pub(super) fn to_big_decimal(document: &DocumentV2) -> Result<BigDecimal, DocumentError> {
    let text = match document {
        DocumentV2::BigDecimal(v) => return Ok(v.clone()),
        DocumentV2::BigInteger(v) => v.as_ref().to_owned(),
        DocumentV2::Number(Number::PosInt(v)) => v.to_string(),
        DocumentV2::Number(Number::NegInt(v)) => v.to_string(),
        DocumentV2::Number(Number::Float(v)) => {
            if !v.is_finite() {
                return Err(non_finite(*v, "bigDecimal"));
            }
            // `Display` prints the shortest decimal that parses back to the same `f64`.
            v.to_string()
        }
        other => return Err(type_mismatch("bigDecimal", other)),
    };
    BigDecimal::from_str(&text).map_err(|e| {
        DocumentError::invalid_input(format!("cannot parse {text:?} as bigDecimal: {e}"))
    })
}

/// Returns the integer part of `value` as `-?[0-9]+`, truncating toward zero.
///
/// The exponent is applied before truncating, so `1.23e10` gives `"12300000000"`, `1.23e1`
/// gives `"12"`, and `-5e-3` gives `"0"`. Returns an overflow error if the integer part has
/// more than [`MAX_INTEGER_DIGITS`] digits.
fn truncate_decimal(value: &BigDecimal, target: &'static str) -> Result<String, DocumentError> {
    let text = value.as_ref();
    let too_large = || DocumentError::numeric_coercion_overflow(target, text);
    let malformed = || DocumentError::invalid_input(format!("{text:?} is not a decimal number"));

    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (coefficient, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((coefficient, exponent)) => match exponent.parse::<i64>() {
            Ok(exponent) => (coefficient, exponent),
            // `BigDecimal` limits the exponent's size, so these arms only guard against a
            // future relaxation of its grammar.
            Err(_) if exponent.starts_with('-') => return Ok("0".to_owned()),
            Err(_) => return Err(too_large()),
        },
        None => (unsigned, 0),
    };
    let (integer_digits, fraction_digits) =
        coefficient.split_once('.').unwrap_or((coefficient, ""));
    if !integer_digits
        .bytes()
        .chain(fraction_digits.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return Err(malformed());
    }

    // The number of digits before the decimal point once the exponent is applied.
    let point = (integer_digits.len() as i64).saturating_add(exponent);
    let digits: String = if point <= 0 {
        String::new()
    } else {
        let point = usize::try_from(point)
            .ok()
            .filter(|point| *point <= MAX_INTEGER_DIGITS)
            .ok_or_else(too_large)?;
        integer_digits
            .chars()
            .chain(fraction_digits.chars())
            .chain(std::iter::repeat('0'))
            .take(point)
            .collect()
    };

    Ok(match digits.trim_start_matches('0') {
        "" => "0".to_owned(),
        digits if negative => format!("-{digits}"),
        digits => digits.to_owned(),
    })
}

/// The source value of a numeric document, for error messages.
fn numeric_text(document: &DocumentV2) -> String {
    match document {
        DocumentV2::Number(Number::PosInt(v)) => v.to_string(),
        DocumentV2::Number(Number::NegInt(v)) => v.to_string(),
        DocumentV2::Number(Number::Float(v)) => v.to_string(),
        DocumentV2::BigInteger(v) => v.as_ref().to_owned(),
        DocumentV2::BigDecimal(v) => v.as_ref().to_owned(),
        other => variant_name(other).to_owned(),
    }
}

fn overflow(target: &'static str, document: &DocumentV2) -> DocumentError {
    DocumentError::numeric_coercion_overflow(target, numeric_text(document))
}

fn non_finite(value: f64, target: &'static str) -> DocumentError {
    DocumentError::invalid_input(format!(
        "cannot convert non-finite number {value} to {target}"
    ))
}

fn type_mismatch(target: &'static str, found: &DocumentV2) -> DocumentError {
    DocumentError::type_mismatch(format!("expected {target}, found {}", variant_name(found)))
}

#[cfg(test)]
mod tests {
    use super::{truncate_decimal, MAX_INTEGER_DIGITS};
    use crate::{BigDecimal, BigInteger, DocumentError, DocumentV2, Number};
    use std::str::FromStr;

    fn big_integer(text: &str) -> DocumentV2 {
        DocumentV2::BigInteger(BigInteger::from_str(text).unwrap())
    }

    fn big_decimal(text: &str) -> DocumentV2 {
        DocumentV2::BigDecimal(BigDecimal::from_str(text).unwrap())
    }

    fn is_overflow(result: Result<impl std::fmt::Debug, DocumentError>) -> bool {
        matches!(result, Err(DocumentError::NumericCoercionOverflow { .. }))
    }

    fn is_type_mismatch(result: Result<impl std::fmt::Debug, DocumentError>) -> bool {
        matches!(result, Err(DocumentError::TypeMismatch { .. }))
    }

    fn is_invalid_input(result: Result<impl std::fmt::Debug, DocumentError>) -> bool {
        matches!(result, Err(DocumentError::InvalidInput { .. }))
    }

    #[test]
    fn integer_accessors_accept_every_numeric_variant() {
        assert_eq!(
            DocumentV2::Number(Number::PosInt(42))
                .coerce_byte()
                .unwrap(),
            42
        );
        assert_eq!(
            DocumentV2::Number(Number::NegInt(-42))
                .coerce_byte()
                .unwrap(),
            -42
        );
        assert_eq!(big_integer("-128").coerce_byte().unwrap(), i8::MIN);
        assert_eq!(big_integer("00123").coerce_short().unwrap(), 123);
        assert_eq!(
            DocumentV2::Number(Number::PosInt(i32::MAX as u64))
                .coerce_integer()
                .unwrap(),
            i32::MAX
        );
        assert_eq!(
            DocumentV2::Number(Number::NegInt(i64::MIN))
                .coerce_long()
                .unwrap(),
            i64::MIN
        );
        assert_eq!(big_decimal("1.23e2").coerce_integer().unwrap(), 123);
    }

    #[test]
    fn integer_accessors_truncate_fractions_toward_zero() {
        assert_eq!(
            DocumentV2::Number(Number::Float(1.9))
                .coerce_integer()
                .unwrap(),
            1
        );
        assert_eq!(
            DocumentV2::Number(Number::Float(-1.9))
                .coerce_integer()
                .unwrap(),
            -1
        );
        assert_eq!(
            DocumentV2::Number(Number::Float(-0.5))
                .coerce_long()
                .unwrap(),
            0
        );
        assert_eq!(big_decimal("100.5").coerce_byte().unwrap(), 100);
        assert_eq!(big_decimal("-127.99").coerce_byte().unwrap(), -127);
        assert_eq!(big_decimal("5e-3").coerce_long().unwrap(), 0);
        assert_eq!(big_decimal(".5").coerce_long().unwrap(), 0);
    }

    #[test]
    fn integer_accessors_report_overflow() {
        assert!(is_overflow(
            DocumentV2::Number(Number::PosInt(128)).coerce_byte()
        ));
        assert!(is_overflow(
            DocumentV2::Number(Number::NegInt(i64::MIN)).coerce_integer()
        ));
        assert!(is_overflow(
            DocumentV2::Number(Number::PosInt(u64::MAX)).coerce_long()
        ));
        assert!(is_overflow(
            DocumentV2::Number(Number::Float(1e20)).coerce_long()
        ));
        assert!(is_overflow(
            DocumentV2::Number(Number::Float(-1e300)).coerce_long()
        ));
        assert!(is_overflow(
            big_integer("12345678901234567890").coerce_long()
        ));
        assert!(is_overflow(
            big_integer("99999999999999999999999999999999999999999999").coerce_long()
        ));
        assert!(is_overflow(big_decimal("128.5").coerce_byte()));
        assert!(is_overflow(big_decimal("1e400").coerce_long()));
        assert!(is_overflow(big_decimal("1e1000000000").coerce_long()));
    }

    #[test]
    fn integer_accessors_reject_non_finite_floats() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(is_invalid_input(
                DocumentV2::Number(Number::Float(value)).coerce_integer()
            ));
        }
    }

    #[test]
    fn numeric_accessors_reject_non_numeric_variants() {
        for document in [
            DocumentV2::String("1".to_owned()),
            DocumentV2::Bool(true),
            DocumentV2::Null,
            DocumentV2::Blob(vec![1]),
            DocumentV2::Array(vec![]),
        ] {
            assert!(is_type_mismatch(document.coerce_long()), "{document:?}");
            assert!(is_type_mismatch(document.coerce_double()), "{document:?}");
            assert!(
                is_type_mismatch(document.coerce_big_integer()),
                "{document:?}"
            );
            assert!(
                is_type_mismatch(document.coerce_big_decimal()),
                "{document:?}"
            );
        }
    }

    #[test]
    fn float_accessors_accept_every_numeric_variant() {
        assert_eq!(
            DocumentV2::Number(Number::PosInt(42))
                .coerce_double()
                .unwrap(),
            42.0
        );
        assert_eq!(
            DocumentV2::Number(Number::NegInt(-42))
                .coerce_double()
                .unwrap(),
            -42.0
        );
        assert_eq!(
            DocumentV2::Number(Number::Float(2.5))
                .coerce_double()
                .unwrap(),
            2.5
        );
        assert_eq!(big_integer("-17").coerce_double().unwrap(), -17.0);
        assert_eq!(big_decimal("1.5E+3").coerce_double().unwrap(), 1500.0);
        assert_eq!(big_decimal("1.5").coerce_float().unwrap(), 1.5_f32);
        // Precision loss is ignored: this rounds to the nearest `f64`.
        assert_eq!(
            big_integer("9007199254740993").coerce_double().unwrap(),
            9_007_199_254_740_992.0
        );
        // So is underflow.
        assert_eq!(big_decimal("1e-400").coerce_double().unwrap(), 0.0);
    }

    #[test]
    fn float_accessors_report_overflow() {
        assert!(is_overflow(big_decimal("1e400").coerce_double()));
        assert!(is_overflow(
            big_integer(&format!("1{}", "0".repeat(400))).coerce_double()
        ));
        assert!(is_overflow(
            DocumentV2::Number(Number::Float(1e300)).coerce_float()
        ));
        assert!(is_overflow(big_decimal("1e39").coerce_float()));
    }

    #[test]
    fn float_accessors_keep_non_finite_floats() {
        assert!(DocumentV2::Number(Number::Float(f64::NAN))
            .coerce_double()
            .unwrap()
            .is_nan());
        assert_eq!(
            DocumentV2::Number(Number::Float(f64::INFINITY))
                .coerce_float()
                .unwrap(),
            f32::INFINITY
        );
    }

    #[test]
    fn coerce_big_integer_accepts_every_numeric_variant() {
        let text =
            |document: DocumentV2| document.coerce_big_integer().unwrap().as_ref().to_owned();
        assert_eq!(
            text(big_integer("-12345678901234567890")),
            "-12345678901234567890"
        );
        assert_eq!(
            text(DocumentV2::Number(Number::PosInt(u64::MAX))),
            u64::MAX.to_string()
        );
        assert_eq!(text(DocumentV2::Number(Number::NegInt(-3))), "-3");
        assert_eq!(text(DocumentV2::Number(Number::Float(1.5))), "1");
        assert_eq!(text(DocumentV2::Number(Number::Float(-1.5))), "-1");
        assert_eq!(text(DocumentV2::Number(Number::Float(-0.5))), "0");
        assert_eq!(
            text(DocumentV2::Number(Number::Float(1e20))),
            "100000000000000000000"
        );
        assert_eq!(text(big_decimal("1.23e10")), "12300000000");
        assert_eq!(text(big_decimal("-9.99")), "-9");
    }

    #[test]
    fn coerce_big_integer_rejects_unrepresentable_values() {
        assert!(is_invalid_input(
            DocumentV2::Number(Number::Float(f64::NAN)).coerce_big_integer()
        ));
        assert!(is_overflow(
            big_decimal("1e1000000000").coerce_big_integer()
        ));
    }

    #[test]
    fn coerce_big_decimal_accepts_every_numeric_variant() {
        let text =
            |document: DocumentV2| document.coerce_big_decimal().unwrap().as_ref().to_owned();
        assert_eq!(text(big_decimal("1.23E-10")), "1.23E-10");
        assert_eq!(
            text(big_integer("-12345678901234567890")),
            "-12345678901234567890"
        );
        assert_eq!(text(DocumentV2::Number(Number::PosInt(7))), "7");
        assert_eq!(text(DocumentV2::Number(Number::NegInt(-7))), "-7");
        assert_eq!(text(DocumentV2::Number(Number::Float(0.1))), "0.1");
        assert_eq!(text(DocumentV2::Number(Number::Float(-2.5))), "-2.5");
        assert!(is_invalid_input(
            DocumentV2::Number(Number::Float(f64::INFINITY)).coerce_big_decimal()
        ));
    }

    #[test]
    fn truncate_decimal_expands_exponents_and_normalizes() {
        for (input, expected) in [
            ("0", "0"),
            ("-0.0", "0"),
            ("+5", "5"),
            ("5.", "5"),
            (".5", "0"),
            ("-.5", "0"),
            ("00123.45", "123"),
            ("1.23e1", "12"),
            ("1.23E10", "12300000000"),
            ("-1.23e+2", "-123"),
            ("5e-3", "0"),
            ("123456e-3", "123"),
        ] {
            let value = BigDecimal::from_str(input).unwrap();
            assert_eq!(
                truncate_decimal(&value, "test").unwrap(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn truncate_decimal_limits_the_number_of_digits() {
        let at_limit = BigDecimal::from_str(&format!("1e{}", MAX_INTEGER_DIGITS - 1)).unwrap();
        assert_eq!(
            truncate_decimal(&at_limit, "test").unwrap().len(),
            MAX_INTEGER_DIGITS
        );
        let over_limit = BigDecimal::from_str(&format!("1e{MAX_INTEGER_DIGITS}")).unwrap();
        assert!(matches!(
            truncate_decimal(&over_limit, "test"),
            Err(DocumentError::NumericCoercionOverflow { .. })
        ));
    }
}
