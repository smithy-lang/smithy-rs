/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A field wrapper that excludes its value from a struct's equality.

use std::fmt;

/// Wraps a value that is carried on a struct but must not participate in the
/// struct's equality.
///
/// Two `EqIgnore<T>` always compare equal regardless of their contents, so a
/// `#[derive(PartialEq)]` struct can hold a value whose type is not (or should
/// not be) `PartialEq` without that value affecting the meaning of `==` over the
/// struct's other fields. This is used to carry out-of-band runtime metadata
/// (for example an `Extensions` type-map) on generated operation outputs.
#[derive(Clone, Default)]
pub(crate) struct EqIgnore<T>(pub(crate) T);

impl<T> EqIgnore<T> {
    /// Borrow the wrapped value.
    #[allow(dead_code)]
    pub(crate) fn get(&self) -> &T {
        &self.0
    }

    /// Mutably borrow the wrapped value.
    #[allow(dead_code)]
    pub(crate) fn get_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> PartialEq for EqIgnore<T> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<T: fmt::Debug> fmt::Debug for EqIgnore<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::EqIgnore;

    #[test]
    fn always_equal_regardless_of_contents() {
        assert_eq!(EqIgnore(1), EqIgnore(2));
        assert_eq!(EqIgnore("a"), EqIgnore("b"));
    }

    #[test]
    fn enclosing_struct_ignores_the_field() {
        #[derive(PartialEq, Debug)]
        struct S {
            modeled: u32,
            ignored: EqIgnore<u32>,
        }
        // Equal modeled field, different ignored field -> equal.
        assert_eq!(
            S {
                modeled: 1,
                ignored: EqIgnore(10)
            },
            S {
                modeled: 1,
                ignored: EqIgnore(20)
            },
        );
        // Different modeled field -> not equal.
        assert_ne!(
            S {
                modeled: 1,
                ignored: EqIgnore(10)
            },
            S {
                modeled: 2,
                ignored: EqIgnore(10)
            },
        );
    }

    #[test]
    fn get_and_get_mut() {
        let mut e = EqIgnore(5);
        assert_eq!(*e.get(), 5);
        *e.get_mut() = 9;
        assert_eq!(*e.get(), 9);
    }
}
