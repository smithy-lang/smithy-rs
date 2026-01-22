/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */

extern crate proc_macro;

use proc_macro::TokenStream;
use syn::parse::Parse;
use syn::Item;

use crate::macro_impl::smithy_metrics_impl;

mod macro_impl;

#[proc_macro_attribute]
pub fn smithy_metrics(attr: TokenStream, input: TokenStream) -> TokenStream {
    let item = syn::parse_macro_input!(input as Item);

    let Item::Struct(item_struct) = item else {
        return syn::Error::new_spanned(item, "expected `struct`")
            .to_compile_error()
            .into();
    };

    let attributes = syn::parse_macro_input!(attr as SmithyMetricsStructAttrs);

    smithy_metrics_impl(attributes, item_struct).into()
}

pub(crate) struct SmithyMetricsStructAttrs {}
impl Parse for SmithyMetricsStructAttrs {
    fn parse(_input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(SmithyMetricsStructAttrs {})
    }
}
