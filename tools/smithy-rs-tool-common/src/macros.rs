/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

/// A macro for attaching info to error messages pointing to the line of code responsible for the error.
/// [Thanks to dtolnay for this macro](https://github.com/dtolnay/anyhow/issues/22#issuecomment-542309452)
#[macro_export]
macro_rules! here {
    () => {
        concat!("error at ", file!(), ":", line!(), ":", column!())
    };
    ($message:tt) => {
        concat!($message, " (", here!(), ")")
    };
}

// export this macro for use in other modules in this crate
pub use here;
