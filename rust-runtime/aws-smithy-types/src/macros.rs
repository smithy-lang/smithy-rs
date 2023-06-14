/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Various utility macros to aid runtime crate writers.

/// Define a new builder struct, along with a method to create it, and setters.
#[macro_export]
macro_rules! builder {
    ($($tt:tt)+) => {
        builder_struct!($($tt)+);

        impl Builder {
            pub fn new() -> Self {
                Builder::default()
            }

            builder_methods!($($tt)+);
        }
    }
}

/// Define a new builder struct, its fields, and their docs
#[macro_export]
macro_rules! builder_struct {
    ($($_setter_name:ident, $field_name:ident, $ty:ty, $doc:literal $(,)?)+) => {
        #[derive(Clone, Debug, Default)]
        pub struct Builder {
            $(
            #[doc = $doc]
            $field_name: Option<$ty>,
            )+
        }
    }
}

/// Define setter methods for a builder struct. Must be called from within an `impl` block.
#[macro_export]
macro_rules! builder_methods {
    ($fn_name:ident, $arg_name:ident, $ty:ty, $doc:literal, $($tail:tt)+) => {
        builder_methods!($fn_name, $arg_name, $ty, $doc);
        builder_methods!($($tail)+);
    };
    ($fn_name:ident, $arg_name:ident, $ty:ty, $doc:literal) => {
        #[doc = $doc]
        pub fn $fn_name(&mut self, $arg_name: Option<$ty>) -> &mut Self {
            self.$arg_name = $arg_name;
            self
        }

        #[doc = $doc]
        pub fn $arg_name(mut self, $arg_name: $ty) -> Self {
            self.$arg_name = Some($arg_name);
            self
        }
    };
}
