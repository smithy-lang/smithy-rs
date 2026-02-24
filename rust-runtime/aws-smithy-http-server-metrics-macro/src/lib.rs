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

    let Item::Struct(mut item_struct) = item else {
        return syn::Error::new_spanned(item, "expected `struct`")
            .to_compile_error()
            .into();
    };

    let current_attrs = syn::parse_macro_input!(attr as SmithyMetricsStructAttrs);
    let combined_attrs = collect_and_merge_attributes(&mut item_struct, current_attrs);

    smithy_metrics_impl(combined_attrs, item_struct).into()
}

/// Collects all `#[smithy_metrics]` attributes from the struct and merges them
/// into a single set of attributes.
///
/// This handles multiple attribute invocations in a single code generation pass:
///
/// ```ignore
/// #[smithy_metrics(rename(service_name = "program"))]
/// struct MyMetrics { ... }
/// ```
///
/// Without this merging, each attribute would trigger the macro separately,
/// generating duplicate code. This function:
/// 1. Collects renames from the current invocation
/// 2. Finds and parses any other `#[smithy_metrics]` attributes on the struct
/// 3. Merges all renames together
/// 4. Removes all `#[smithy_metrics]` attributes from the struct
/// 5. Returns combined attributes for single code generation
fn collect_and_merge_attributes(
    item_struct: &mut syn::ItemStruct,
    current_attrs: SmithyMetricsStructAttrs,
) -> SmithyMetricsStructAttrs {
    let mut all_renames = current_attrs.renames;
    let mut remaining_attrs = Vec::new();

    for attr in item_struct.attrs.drain(..) {
        if attr.path().is_ident("smithy_metrics") {
            // Parse and merge other smithy_metrics attributes
            if let Ok(parsed) = attr.parse_args::<SmithyMetricsStructAttrs>() {
                all_renames.extend(parsed.renames);
            }
            // Don't add smithy_metrics attributes to remaining_attrs - this prevents
            // subsequent macro invocations from triggering
        } else {
            // Keep all other attributes (like #[derive], #[metrics], etc.)
            remaining_attrs.push(attr);
        }
    }

    // Update struct with only non-smithy_metrics attributes
    item_struct.attrs = remaining_attrs;

    SmithyMetricsStructAttrs {
        renames: all_renames,
    }
}

/// Attributes parsed from `#[smithy_metrics(...)]` macro invocations.
///
/// Currently supports:
/// - `rename(key = "value", ...)` - Rename metric fields
///
/// # Examples
///
/// ```ignore
/// #[smithy_metrics(rename(service_name = "program"))]
/// struct MyMetrics { ... }
/// ```
#[derive(Debug, Default)]
pub(crate) struct SmithyMetricsStructAttrs {
    /// List of rename mappings specified in the attribute.
    pub(crate) renames: Vec<Rename>,
}

impl Parse for SmithyMetricsStructAttrs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Handle empty attributes: `#[smithy_metrics]`
        if input.is_empty() {
            return Ok(SmithyMetricsStructAttrs::default());
        }

        let mut renames = Vec::new();

        // Parse comma-separated attribute items like: rename(...), other(...)
        let parsed = input.parse_terminated(parse_rename_meta, syn::Token![,])?;
        for item in parsed {
            renames.extend(item);
        }

        Ok(SmithyMetricsStructAttrs { renames })
    }
}

/// Parses a single `rename(...)` attribute.
///
/// Expected syntax: `rename(key1 = "value1", key2 = "value2", ...)`
///
/// Returns a vector of rename mappings extracted from the parentheses.
fn parse_rename_meta(input: syn::parse::ParseStream) -> syn::Result<Vec<Rename>> {
    let ident: syn::Ident = input.parse()?;
    if ident != "rename" {
        return Err(syn::Error::new_spanned(
            &ident,
            format!("unknown smithy_metrics attribute `{}`", ident),
        ));
    }

    // Parse the parenthesized content: rename(...)
    let content;
    syn::parenthesized!(content in input);

    // Ensure at least one mapping is provided
    if content.is_empty() {
        return Err(syn::Error::new(
            content.span(),
            "expected at least one rename mapping",
        ));
    }

    // Parse comma-separated key-value pairs: key = "value", ...
    let mut renames = Vec::new();
    while !content.is_empty() {
        let from: syn::Ident = content.parse()?;
        content.parse::<syn::Token![=]>()?;
        let to: syn::LitStr = content.parse()?;
        renames.push(Rename {
            from: from.to_string(),
            to: to.value(),
        });

        if !content.is_empty() {
            content.parse::<syn::Token![,]>()?;
        }
    }

    Ok(renames)
}

/// Represents a single rename mapping from one name to another.
///
/// Used in `#[smithy_metrics(rename("from" = "to"))]` attributes to
/// rename metric field names during code generation.
#[derive(Debug)]
pub(crate) struct Rename {
    /// The original name to be renamed.
    pub(crate) from: String,
    /// The new name to use.
    pub(crate) to: String,
}
