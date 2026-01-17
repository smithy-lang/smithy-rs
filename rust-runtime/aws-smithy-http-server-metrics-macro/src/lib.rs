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
use syn::Ident;
use syn::Item;
use syn::Token;

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

pub(crate) struct SmithyMetricsStructAttrs {
    pub server_crate: Ident,
}
impl Parse for SmithyMetricsStructAttrs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "expected attribute arguments in parentheses: `smithy_metrics(server_crate = ...)`",
            ));
        }

        let name: Ident = input.parse()?;
        let name_str = name.to_string();

        if name_str != "server_crate" {
            return Err(syn::Error::new_spanned(
                name,
                format!(
                    "unknown parameter `{}`\nhelp: expected `server_crate`",
                    name_str
                ),
            ));
        }

        let _assign_token: Token![=] = input.parse()?;
        let server_crate: Ident = input.parse().map_err(|e| {
            syn::Error::new(e.span(), format!("expected identifier for server_crate value: {}", e))
        })?;

        Ok(SmithyMetricsStructAttrs { server_crate })
    }
}
