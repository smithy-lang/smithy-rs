/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Coalesce function that returns the first non-None value.
/// This is a placeholder that will be called by generated code.
/// The actual implementation is generated inline based on the number of arguments.

/// Helper trait to implement the coalesce! macro
pub(crate) trait Coalesce {
    /// The first arg
    type Arg1;
    /// The second arg
    type Arg2;
    /// The result of comparing Arg1 and Arg1
    type Result;

    /// Evaluates arguments in order and returns the first non-empty result, otherwise returns the result of the last argument.
    fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result;
}

impl<T> Coalesce for &&&(&Option<T>, &Option<T>) {
    type Arg1 = Option<T>;
    type Arg2 = Option<T>;
    type Result = Option<T>;

    fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result {
        |a: Option<T>, b: Option<T>| a.or(b)
    }
}

impl<T> Coalesce for &&(&Option<T>, &T) {
    type Arg1 = Option<T>;
    type Arg2 = T;
    type Result = T;

    fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result {
        |a: Option<T>, b: T| a.unwrap_or(b)
    }
}

impl<T, U> Coalesce for &(&T, &U) {
    type Arg1 = T;
    type Arg2 = U;
    type Result = T;

    fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result {
        |a: T, _b| a
    }
}

/// Evaluates arguments in order and returns the first non-empty result, otherwise returns the result of the last argument.
macro_rules! coalesce {
    ($a:expr) => {$a};
    ($a:expr, $b:expr) => {{
        use crate::endpoint_lib::coalesce::Coalesce;
        let a = $a;
        let b = $b;
        (&&&(&a, &b)).coalesce()(a, b)
    }};
    ($a:expr, $b:expr $(, $c:expr)* $(,)?) => {
        $crate::endpoint_lib::coalesce::coalesce!($crate::endpoint_lib::coalesce::coalesce!($a, $b) $(, $c)*)
    }
}

pub(crate) use coalesce;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn base_cases() {
        // All types optional
        let a = Some("a");
        let b = Some("b");

        assert_eq!(coalesce!(a, b), Some("a"));

        let a = None;
        let b = Some("b");

        assert_eq!(coalesce!(a, b), Some("b"));

        let a = Some("a");
        let b = None;

        assert_eq!(coalesce!(a, b), Some("a"));

        let a: Option<&str> = None;
        let b: Option<&str> = None;

        assert_eq!(coalesce!(a, b), None);

        // Some non-optional types
        let a = "a";
        let b = Some("b");

        assert_eq!(coalesce!(a, b), "a");

        let a = Some("a");
        let b = "b";

        assert_eq!(coalesce!(a, b), "a");

        let a = None;
        let b = "b";

        assert_eq!(coalesce!(a, b), "b");

        let a = "a";
        let b: Option<&str> = None;

        assert_eq!(coalesce!(a, b), "a");

        // All types non-optional
        let a = "a";
        let b = "b";

        assert_eq!(coalesce!(a, b), "a");

        // Works with trailing comma (makes codegen easier)
        let a = "a";
        let b = "b";

        assert_eq!(coalesce!(a, b,), "a");
    }

    #[test]
    fn longer_cases() {
        assert_eq!(
            coalesce!(None, None, None, None, None, None, None, None, None, None, "a"),
            "a"
        );

        // In the generated code all of the inputs are typed variables, so the turbofish isn't needed in practice
        assert_eq!(
            coalesce!(
                None,
                None,
                None,
                None,
                "a",
                None::<&str>,
                None::<&str>,
                None::<&str>,
                None::<&str>,
                None::<&str>,
                None::<&str>
            ),
            "a"
        );

        assert_eq!(
            coalesce!(
                None,
                None,
                None,
                None,
                "a",
                None::<&str>,
                Some("b"),
                None::<&str>,
                None::<&str>,
                None::<&str>,
                "c"
            ),
            "a"
        );

        assert_eq!(
            coalesce!(
                None,
                None,
                None,
                Some("a"),
                None,
                None,
                None,
                None,
                None,
                None,
                "b"
            ),
            "a"
        );
        assert_eq!(
            coalesce!(
                Some("a"),
                None,
                None,
                Some("b"),
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            Some("a")
        );

        assert_eq!(coalesce!("a", "b", "c", "d", "e", "f", "g",), "a");

        assert_eq!(
            coalesce!(
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None::<&str>,
            ),
            None
        );
    }
}
