/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Path, Token};

/// Parse descriptor attributes separately from structure/member attributes.
pub(crate) fn descriptor(input: DeriveInput, service: bool) -> syn::Result<TokenStream> {
    let name = &input.ident;
    if !input.generics.params.is_empty()
        || !matches!(&input.data, Data::Struct(s) if matches!(s.fields, Fields::Unit))
    {
        return Err(syn::Error::new_spanned(
            name,
            "schema descriptors require a unit struct without generics",
        ));
    }
    let mut namespace: Option<LitStr> = None;
    let mut shape_name: Option<LitStr> = None;
    let mut version: Option<LitStr> = None;
    let mut input_type: Option<Path> = None;
    let mut output_type: Option<Path> = None;
    let mut errors: Vec<Path> = Vec::new();
    let mut operations: Vec<Path> = Vec::new();
    let mut protocols: Vec<LitStr> = Vec::new();
    for attr in &input.attrs {
        if !attr.path().is_ident("smithy") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("target") {
                let value: LitStr = meta.value()?.parse()?;
                if crate::Target::parse(&value)? != crate::Target::Server {
                    return Err(syn::Error::new_spanned(value, "operation/service derives currently use server runtime descriptors; target must be server"));
                }
            } else if meta.path.is_ident("namespace") {
                namespace = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("shape_name") {
                shape_name = Some(meta.value()?.parse()?);
            } else if service && meta.path.is_ident("version") {
                version = Some(meta.value()?.parse()?);
            } else if !service && meta.path.is_ident("input") {
                input_type = Some(meta.value()?.parse()?);
            } else if !service && meta.path.is_ident("output") {
                output_type = Some(meta.value()?.parse()?);
            } else if !service && meta.path.is_ident("errors") {
                let content;
                syn::parenthesized!(content in meta.input);
                errors.extend(content.parse_terminated(Path::parse_mod_style, Token![,])?);
            } else if service && meta.path.is_ident("operations") {
                let content;
                syn::parenthesized!(content in meta.input);
                operations.extend(content.parse_terminated(Path::parse_mod_style, Token![,])?);
            } else if service && meta.path.is_ident("protocols") {
                let content;
                syn::parenthesized!(content in meta.input);
                protocols
                    .extend(content.parse_terminated(|input| input.parse::<LitStr>(), Token![,])?);
            } else {
                return Err(meta.error("unsupported schema descriptor attribute"));
            }
            Ok(())
        })?;
    }
    let namespace =
        namespace.ok_or_else(|| syn::Error::new_spanned(name, "missing smithy namespace"))?;
    let shape_name = shape_name.unwrap_or_else(|| LitStr::new(&name.to_string(), name.span()));
    let id = quote!(::aws_smithy_schema::shape_id!(#namespace, #shape_name));
    let (ty, value) = if service {
        let protocol_ids = protocols
            .iter()
            .map(|literal| {
                let value = literal.value();
                let Some((ns, shape)) = value.split_once('#') else {
                    return Err(syn::Error::new_spanned(
                        literal,
                        "protocol must be a fully qualified shape ID",
                    ));
                };
                if ns.is_empty() || shape.is_empty() || shape.contains(['#', '$']) {
                    return Err(syn::Error::new_spanned(
                        literal,
                        "protocol must identify a shape, not a member",
                    ));
                }
                Ok(quote!(::aws_smithy_schema::shape_id!(#ns, #shape)))
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let version = match version {
            Some(v) => quote!(::std::option::Option::Some(#v)),
            None => quote!(::std::option::Option::None),
        };
        (
            quote!(::aws_smithy_http_server::schema::ServiceSchema<'static>),
            quote! {
                ::aws_smithy_http_server::schema::ServiceSchema::new(#id, #version,
                    &[#(#protocol_ids,)*], &[#(#operations::SCHEMA,)*])
            },
        )
    } else {
        let input_type =
            input_type.ok_or_else(|| syn::Error::new_spanned(name, "missing smithy input type"))?;
        let output_type = output_type
            .ok_or_else(|| syn::Error::new_spanned(name, "missing smithy output type"))?;
        (
            quote!(::aws_smithy_http_server::schema::OperationSchema<'static>),
            quote! {
                ::aws_smithy_http_server::schema::OperationSchema::new(#id,
                    #input_type::SCHEMA, #output_type::SCHEMA, &[#(#errors::SCHEMA,)*])
            },
        )
    };
    Ok(quote! {
        impl #name {
            /// The schema descriptor for this shape.
            pub const SCHEMA: &'static #ty = &#value;
        }
    })
}

pub(crate) fn error(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "SmithyError requires an enum",
        ));
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "SmithyError does not support generics",
        ));
    }
    let mut target = crate::Target::Shared;
    let mut serialize = true;
    for attr in &input.attrs {
        if !attr.path().is_ident("smithy") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("target") {
                target = crate::Target::parse(&meta.value()?.parse::<LitStr>()?)?;
            } else if meta.path.is_ident("serialize") {
                serialize = meta.value()?.parse::<syn::LitBool>()?.value;
            } else {
                return Err(meta.error("SmithyError accepts only target and serialize"));
            }
            Ok(())
        })?;
    }
    let mut variants = Vec::new();
    for variant in &data.variants {
        if !matches!(&variant.fields, Fields::Unnamed(fields) if fields.unnamed.len() == 1) {
            return Err(syn::Error::new_spanned(
                variant,
                "each error variant must contain one modeled error",
            ));
        }
        variants.push(&variant.ident);
    }
    let serialization_impl = serialize.then(|| quote! {
        impl ::aws_smithy_schema::serde::SerializableStruct for #name {
            fn serialize_members(&self, ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer)
                -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
                match self { #(Self::#variants(inner) => ::aws_smithy_schema::serde::SerializableStruct::serialize_members(inner, ser),)* }
            }
        }
    });
    let http_impl = (serialize && target == crate::Target::Server).then(|| quote! {
        impl ::aws_smithy_http_server::schema::HttpModeledError for #name {
            fn status_code(&self) -> u16 {
                match self { #(Self::#variants(inner) => ::aws_smithy_http_server::schema::HttpModeledError::status_code(inner),)* }
            }
        }    });
    Ok(quote! {
        impl ::std::fmt::Display for #name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self { #(Self::#variants(inner) => ::std::fmt::Display::fmt(inner, f),)* }
            }
        }
        impl ::std::error::Error for #name {
            fn source(&self) -> ::std::option::Option<&(dyn ::std::error::Error + 'static)> {
                match self { #(Self::#variants(inner) => ::std::option::Option::Some(inner),)* }
            }
        }
        #serialization_impl
        #http_impl
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn invalid_descriptor_declarations_have_diagnostics() {
        for input in [
            parse_quote!(
                #[smithy(namespace = "example", input = Input)]
                struct MissingOutput;
            ),
            parse_quote!(
                #[smithy(namespace = "example", target = "client", input = Input, output = Output)]
                struct Client;
            ),
            parse_quote!(
                #[smithy(namespace = "example", input = Input, output = Output)]
                struct Generic<T>(T);
            ),
        ] {
            assert!(descriptor(input, false).is_err());
        }
        for protocol in ["no_namespace", "#Empty", "example#Protocol$member"] {
            let input = syn::parse2(quote!(
                #[smithy(namespace = "example", protocols(#protocol))]
                struct Service;
            ))
            .unwrap();
            assert!(descriptor(input, true).is_err());
        }
        assert!(error(parse_quote!(
            enum Invalid {
                Unit,
            }
        ))
        .is_err());
        assert!(error(parse_quote!(
            #[smithy(target = "invalid")]
            enum Invalid {
                Error(E),
            }
        ))
        .is_err());
        assert!(error(parse_quote!(
            #[smithy(serialize = "false")]
            enum Invalid {
                Error(E),
            }
        ))
        .is_err());
    }
}
