/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[macro_export]
macro_rules! assert_eq_f64 {
    ($left:expr, $right:expr $(,)?) => {
        if ($left - $right).abs() > f64::EPSILON {
            panic!("assertion failed: `(left_f64 == right_f64)`\n\tleft:\t`{}`\n\tright:\t`{}`", $left, $right);
        }
    };
    ($left:expr, $right:expr, $($arg:tt)+) => {
        if ($left - $right).abs() > f64::EPSILON {
            panic!(
                "assertion failed: `(left_f64 == right_f64)`\n\tleft:\t`{}`\n\tright:\t`{}`: {}", $left, $right, format_args!($($arg)+));
        }
    };
}
