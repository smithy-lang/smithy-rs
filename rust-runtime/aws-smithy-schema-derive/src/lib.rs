/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Derive macro for hand-written [`aws-smithy-schema`](https://crates.io/crates/aws-smithy-schema)
//! shapes.
//!
//! Use through the `derive` feature of `aws-smithy-schema` rather than depending on this
//! crate directly:
//!
//! ```toml
//! aws-smithy-schema = { version = "...", features = ["derive"] }
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::Parse;
use syn::{
    parse_macro_input, Data, DeriveInput, Expr, Fields, GenericArgument, Ident, LitInt, LitStr,
    PathArguments, Token, Type,
};

/// Derives a Smithy schema and serialization support for a hand-written struct.
///
/// Generates, for the annotated struct:
///
/// - A `Schema` static for the struct and one for each (non-skipped) field, exposed as the
///   associated constant `Self::SCHEMA` — the same convention used by smithy-rs generated code,
///   so hand-written and generated shapes nest freely in either direction.
/// - An `aws_smithy_schema::serde::SerializableStruct` implementation.
/// - For `@error` shapes (`#[smithy(error = "...")]`): `Display`, `std::error::Error`, and
///   `HttpModeledError` implementations. The last comes from `aws-smithy-http-server`, which
///   must be a direct dependency of the deriving crate.
///
/// # Container attributes
///
/// - `#[smithy(namespace = "com.example")]` — **required.** The Smithy namespace of the shape.
/// - `#[smithy(shape_name = "OtherName")]` — overrides the shape name (default: struct name).
/// - `#[smithy(error = "client")]` / `#[smithy(error = "server")]` — marks the shape as a Smithy
///   `@error` and generates the error trait implementations listed above.
/// - `#[smithy(http_error = 404)]` — the `@httpError` status code. Requires `error`.
///   Defaults to `400` for client errors and `500` for server errors.
/// - `#[smithy(no_display)]` — suppresses the generated `Display`/`std::error::Error`
///   implementations for an error shape so you can write your own.
/// - `#[smithy(sensitive)]`, `#[smithy(xml_name = "...")]` — common shape-level traits.
/// - `#[smithy(traits(expr, ...))]` — attaches arbitrary trait values. Each expression must
///   evaluate to a type implementing `aws_smithy_schema::Trait`; it is boxed into the shape's
///   trait map.
///
/// # Field attributes
///
/// - `#[smithy(skip)]` — excludes the field from the schema and from serialization.
/// - `#[smithy(rename = "wireName")]` — the Smithy member name (default: the field name).
/// - `#[smithy(sensitive)]`, `#[smithy(json_name = "...")]`, `#[smithy(xml_name = "...")]`,
///   `#[smithy(xml_attribute)]`, `#[smithy(xml_flattened)]`, `#[smithy(http_header = "...")]`,
///   `#[smithy(http_query = "...")]`, `#[smithy(http_label)]`, `#[smithy(http_payload)]`,
///   `#[smithy(http_prefix_headers = "...")]`, `#[smithy(media_type = "...")]`,
///   `#[smithy(timestamp_format = "date-time" | "epoch-seconds" | "http-date")]` — the
///   corresponding Smithy member traits.
/// - `#[smithy(traits(expr, ...))]` — arbitrary trait values for the member, as above.
///
/// # Supported field types
///
/// `bool`, `i8`, `i16`, `i32`, `i64`, `f32`, `f64`, `String`, `Blob`, `DateTime`, `Document`,
/// `BigInteger`, `BigDecimal`, `Vec<String>`, `Vec<i32>`, `Vec<i64>`, `Vec<Blob>`,
/// `HashMap<String, String>`, and `Option<T>` of any of these (optional members are omitted
/// when `None`). Any other path type is treated as a nested structure: it must expose a
/// `SCHEMA` associated constant and implement `SerializableStruct` — which both this derive
/// and smithy-rs generated code provide. `Vec<T>` of such a type serializes as a list of
/// structures.
///
/// # Example
///
/// ```ignore
/// use aws_smithy_schema::SmithySchema;
///
/// #[derive(Debug, SmithySchema)]
/// #[smithy(namespace = "pokemon_service.authz", error = "client", http_error = 401)]
/// pub struct AuthorizeError {
///     pub message: String,
/// }
/// ```
#[proc_macro_derive(SmithySchema, attributes(smithy))]
pub fn derive_smithy_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

// ===========================================================================
// Attribute models
// ===========================================================================

#[derive(Default)]
struct ContainerArgs {
    namespace: Option<LitStr>,
    shape_name: Option<LitStr>,
    error: Option<LitStr>,
    http_error: Option<LitInt>,
    no_display: bool,
    sensitive: bool,
    xml_name: Option<LitStr>,
    traits: Vec<Expr>,
}

#[derive(Default)]
struct FieldArgs {
    skip: bool,
    rename: Option<LitStr>,
    sensitive: bool,
    json_name: Option<LitStr>,
    xml_name: Option<LitStr>,
    xml_attribute: bool,
    xml_flattened: bool,
    http_header: Option<LitStr>,
    http_query: Option<LitStr>,
    http_label: bool,
    http_payload: bool,
    http_prefix_headers: Option<LitStr>,
    media_type: Option<LitStr>,
    timestamp_format: Option<LitStr>,
    traits: Vec<Expr>,
}

fn parse_container_args(input: &DeriveInput) -> syn::Result<ContainerArgs> {
    let mut args = ContainerArgs::default();
    for attr in &input.attrs {
        if !attr.path().is_ident("smithy") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("namespace") {
                args.namespace = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("shape_name") {
                args.shape_name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("error") {
                let lit: LitStr = meta.value()?.parse()?;
                if lit.value() != "client" && lit.value() != "server" {
                    return Err(syn::Error::new_spanned(
                        &lit,
                        "`error` must be \"client\" or \"server\"",
                    ));
                }
                args.error = Some(lit);
            } else if meta.path.is_ident("http_error") {
                args.http_error = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("no_display") {
                args.no_display = true;
            } else if meta.path.is_ident("sensitive") {
                args.sensitive = true;
            } else if meta.path.is_ident("xml_name") {
                args.xml_name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("traits") {
                let content;
                syn::parenthesized!(content in meta.input);
                for expr in content.parse_terminated(Expr::parse, Token![,])? {
                    args.traits.push(expr);
                }
            } else {
                return Err(meta.error(
                    "unknown container attribute; expected one of: `namespace`, `shape_name`, \
                     `error`, `http_error`, `no_display`, `sensitive`, `xml_name`, `traits(...)`",
                ));
            }
            Ok(())
        })?;
    }
    Ok(args)
}

fn parse_field_args(field: &syn::Field) -> syn::Result<FieldArgs> {
    let mut args = FieldArgs::default();
    for attr in &field.attrs {
        if !attr.path().is_ident("smithy") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                args.skip = true;
            } else if meta.path.is_ident("rename") {
                args.rename = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("sensitive") {
                args.sensitive = true;
            } else if meta.path.is_ident("json_name") {
                args.json_name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("xml_name") {
                args.xml_name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("xml_attribute") {
                args.xml_attribute = true;
            } else if meta.path.is_ident("xml_flattened") {
                args.xml_flattened = true;
            } else if meta.path.is_ident("http_header") {
                args.http_header = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("http_query") {
                args.http_query = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("http_label") {
                args.http_label = true;
            } else if meta.path.is_ident("http_payload") {
                args.http_payload = true;
            } else if meta.path.is_ident("http_prefix_headers") {
                args.http_prefix_headers = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("media_type") {
                args.media_type = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("timestamp_format") {
                args.timestamp_format = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("traits") {
                let content;
                syn::parenthesized!(content in meta.input);
                for expr in content.parse_terminated(Expr::parse, Token![,])? {
                    args.traits.push(expr);
                }
            } else {
                return Err(meta.error(
                    "unknown field attribute; expected one of: `skip`, `rename`, `sensitive`, \
                     `json_name`, `xml_name`, `xml_attribute`, `xml_flattened`, `http_header`, \
                     `http_query`, `http_label`, `http_payload`, `http_prefix_headers`, \
                     `media_type`, `timestamp_format`, `traits(...)`",
                ));
            }
            Ok(())
        })?;
    }
    Ok(args)
}

// ===========================================================================
// Field type classification
// ===========================================================================

/// The value a member serializes as, after unwrapping `Option`.
enum ValueKind {
    Boolean,
    Byte,
    Short,
    Integer,
    Long,
    Float,
    Double,
    String,
    Blob,
    Timestamp,
    Document,
    BigInteger,
    BigDecimal,
    StringList,
    IntegerList,
    LongList,
    BlobList,
    StructList(Type),
    StringStringMap,
    Struct,
}

/// Returns the last path segment's identifier and generic arguments, if `ty` is a path type.
fn path_segment(ty: &Type) -> Option<(&Ident, Vec<&Type>)> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    let args = match &segment.arguments {
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter_map(|arg| match arg {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        PathArguments::None => Vec::new(),
        PathArguments::Parenthesized(_) => return None,
    };
    Some((&segment.ident, args))
}

/// Unwraps `Option<T>` to `(true, T)`; returns `(false, ty)` otherwise.
fn unwrap_option(ty: &Type) -> (bool, &Type) {
    if let Some((ident, args)) = path_segment(ty) {
        if ident == "Option" && args.len() == 1 {
            return (true, args[0]);
        }
    }
    (false, ty)
}

fn classify(ty: &Type, field: &syn::Field) -> syn::Result<ValueKind> {
    let unsupported = |detail: &str| {
        syn::Error::new_spanned(
            &field.ty,
            format!(
                "unsupported field type for #[derive(SmithySchema)]: {detail}. \
                 Mark the field with `#[smithy(skip)]` to exclude it from the schema."
            ),
        )
    };
    let Some((ident, args)) = path_segment(ty) else {
        return Err(unsupported("expected a named type"));
    };
    let kind = match ident.to_string().as_str() {
        "bool" => ValueKind::Boolean,
        "i8" => ValueKind::Byte,
        "i16" => ValueKind::Short,
        "i32" => ValueKind::Integer,
        "i64" => ValueKind::Long,
        "f32" => ValueKind::Float,
        "f64" => ValueKind::Double,
        "String" => ValueKind::String,
        "Blob" => ValueKind::Blob,
        "DateTime" => ValueKind::Timestamp,
        "Document" => ValueKind::Document,
        "BigInteger" => ValueKind::BigInteger,
        "BigDecimal" => ValueKind::BigDecimal,
        "Vec" => {
            let [elem] = args.as_slice() else {
                return Err(unsupported("`Vec` must have exactly one type parameter"));
            };
            let Some((elem_ident, elem_args)) = path_segment(elem) else {
                return Err(unsupported("unsupported `Vec` element type"));
            };
            match elem_ident.to_string().as_str() {
                "String" => ValueKind::StringList,
                "i32" => ValueKind::IntegerList,
                "i64" => ValueKind::LongList,
                "Blob" => ValueKind::BlobList,
                "u8" => {
                    return Err(unsupported(
                        "`Vec<u8>` — use `aws_smithy_types::Blob` for binary data",
                    ))
                }
                "Vec" | "Option" | "HashMap" => {
                    return Err(unsupported("nested collections are not supported"))
                }
                _ if elem_args.is_empty() => ValueKind::StructList((*elem).clone()),
                _ => return Err(unsupported("unsupported `Vec` element type")),
            }
        }
        "HashMap" => {
            let is_string =
                |t: &Type| path_segment(t).is_some_and(|(i, a)| i == "String" && a.is_empty());
            match args.as_slice() {
                [k, v] if is_string(k) && is_string(v) => ValueKind::StringStringMap,
                _ => {
                    return Err(unsupported(
                        "only `HashMap<String, String>` maps are supported",
                    ))
                }
            }
        }
        "Option" => return Err(unsupported("nested `Option` is not supported")),
        _ => ValueKind::Struct,
    };
    Ok(kind)
}

impl ValueKind {
    fn shape_type(&self) -> TokenStream2 {
        let variant = match self {
            ValueKind::Boolean => "Boolean",
            ValueKind::Byte => "Byte",
            ValueKind::Short => "Short",
            ValueKind::Integer => "Integer",
            ValueKind::Long => "Long",
            ValueKind::Float => "Float",
            ValueKind::Double => "Double",
            ValueKind::String => "String",
            ValueKind::Blob => "Blob",
            ValueKind::Timestamp => "Timestamp",
            ValueKind::Document => "Document",
            ValueKind::BigInteger => "BigInteger",
            ValueKind::BigDecimal => "BigDecimal",
            ValueKind::StringList
            | ValueKind::IntegerList
            | ValueKind::LongList
            | ValueKind::BlobList
            | ValueKind::StructList(_) => "List",
            ValueKind::StringStringMap => "Map",
            ValueKind::Struct => "Structure",
        };
        let ident = format_ident!("{variant}");
        quote! { ::aws_smithy_schema::ShapeType::#ident }
    }

    /// The `ser.write_*` statement for this member. `val` is a `&T` binding.
    fn write_stmt(&self, member: &Ident) -> TokenStream2 {
        match self {
            ValueKind::Boolean => quote! { ser.write_boolean(&#member, *val)?; },
            ValueKind::Byte => quote! { ser.write_byte(&#member, *val)?; },
            ValueKind::Short => quote! { ser.write_short(&#member, *val)?; },
            ValueKind::Integer => quote! { ser.write_integer(&#member, *val)?; },
            ValueKind::Long => quote! { ser.write_long(&#member, *val)?; },
            ValueKind::Float => quote! { ser.write_float(&#member, *val)?; },
            ValueKind::Double => quote! { ser.write_double(&#member, *val)?; },
            ValueKind::String => quote! { ser.write_string(&#member, val)?; },
            // Refcount bump, not a payload copy: `Blob` wraps `bytes::Bytes`.
            ValueKind::Blob => quote! { ser.write_blob(&#member, val.clone())?; },
            ValueKind::Timestamp => quote! { ser.write_timestamp(&#member, val)?; },
            ValueKind::Document => quote! { ser.write_document(&#member, val)?; },
            ValueKind::BigInteger => quote! { ser.write_big_integer(&#member, val)?; },
            ValueKind::BigDecimal => quote! { ser.write_big_decimal(&#member, val)?; },
            ValueKind::StringList => quote! { ser.write_string_list(&#member, val)?; },
            ValueKind::IntegerList => quote! { ser.write_integer_list(&#member, val)?; },
            ValueKind::LongList => quote! { ser.write_long_list(&#member, val)?; },
            ValueKind::BlobList => quote! { ser.write_blob_list(&#member, val)?; },
            ValueKind::StructList(elem) => quote! {
                ser.write_list(
                    &#member,
                    &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for item in val {
                            ser.write_struct(<#elem>::SCHEMA, item)?;
                        }
                        Ok(())
                    },
                )?;
            },
            ValueKind::StringStringMap => quote! { ser.write_string_string_map(&#member, val)?; },
            ValueKind::Struct => quote! { ser.write_struct(&#member, val)?; },
        }
    }
}

// ===========================================================================
// Expansion
// ===========================================================================

fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "#[derive(SmithySchema)] does not support generic types: schemas are 'static",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "#[derive(SmithySchema)] only supports structs",
        ));
    };
    let fields: Vec<&syn::Field> = match &data.fields {
        Fields::Named(named) => named.named.iter().collect(),
        Fields::Unit => Vec::new(),
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "#[derive(SmithySchema)] does not support tuple structs",
            ));
        }
    };

    let container = parse_container_args(&input)?;
    let Some(namespace) = container.namespace.clone() else {
        return Err(syn::Error::new_spanned(
            name,
            "missing `#[smithy(namespace = \"...\")]` attribute",
        ));
    };
    if container.http_error.is_some() && container.error.is_none() {
        return Err(syn::Error::new_spanned(
            name,
            "`http_error` requires `#[smithy(error = \"client\" | \"server\")]`",
        ));
    }
    let ns = namespace.value();
    let shape_name = container
        .shape_name
        .as_ref()
        .map(LitStr::value)
        .unwrap_or_else(|| name.to_string());
    let shape_name_lit = LitStr::new(&shape_name, name.span());
    let ns_lit = LitStr::new(&ns, namespace.span());

    // --- Members ---
    let mut statics = Vec::new();
    let mut member_refs = Vec::new();
    let mut serialize_stmts = Vec::new();
    let mut member_index: usize = 0;

    for field in &fields {
        let field_args = parse_field_args(field)?;
        if field_args.skip {
            continue;
        }
        let field_ident = field.ident.as_ref().expect("named fields checked above");
        let member_name = field_args
            .rename
            .as_ref()
            .map(LitStr::value)
            .unwrap_or_else(|| field_ident.to_string());
        let member_name_lit = LitStr::new(&member_name, field_ident.span());
        let member_static = format_ident!("MEMBER_{member_index}");

        let (is_optional, value_ty) = unwrap_option(&field.ty);
        let kind = classify(value_ty, field)?;

        // Trait chain from typed setters.
        let mut chain = TokenStream2::new();
        if field_args.sensitive {
            chain.extend(quote! { .with_sensitive() });
        }
        if let Some(v) = &field_args.json_name {
            chain.extend(quote! { .with_json_name(#v) });
        }
        if let Some(v) = &field_args.xml_name {
            chain.extend(quote! { .with_xml_name(#v) });
        }
        if field_args.xml_attribute {
            chain.extend(quote! { .with_xml_attribute() });
        }
        if field_args.xml_flattened {
            chain.extend(quote! { .with_xml_flattened() });
        }
        if let Some(v) = &field_args.http_header {
            chain.extend(quote! { .with_http_header(#v) });
        }
        if let Some(v) = &field_args.http_query {
            chain.extend(quote! { .with_http_query(#v) });
        }
        if field_args.http_label {
            chain.extend(quote! { .with_http_label() });
        }
        if field_args.http_payload {
            chain.extend(quote! { .with_http_payload() });
        }
        if let Some(v) = &field_args.http_prefix_headers {
            chain.extend(quote! { .with_http_prefix_headers(#v) });
        }
        if let Some(v) = &field_args.media_type {
            chain.extend(quote! { .with_media_type(#v) });
        }
        if let Some(v) = &field_args.timestamp_format {
            let variant = match v.value().as_str() {
                "date-time" => format_ident!("DateTime"),
                "epoch-seconds" => format_ident!("EpochSeconds"),
                "http-date" => format_ident!("HttpDate"),
                other => {
                    return Err(syn::Error::new_spanned(
                        v,
                        format!(
                            "unknown timestamp format {other:?}; expected \"date-time\", \
                             \"epoch-seconds\", or \"http-date\""
                        ),
                    ));
                }
            };
            chain.extend(quote! {
                .with_timestamp_format(::aws_smithy_schema::traits::TimestampFormat::#variant)
            });
        }

        // Aggregate/nested member wiring.
        match &kind {
            ValueKind::StructList(_) => {
                // A synthetic list shape holds the element member, mirroring how
                // codegen models `list Foo { member: Bar }`.
                let elem_static = format_ident!("MEMBER_{member_index}_ELEM");
                let list_shape = format!(
                    "{shape_name}{}List",
                    upper_camel_case(&field_ident.to_string())
                );
                let list_shape_lit = LitStr::new(&list_shape, field_ident.span());
                statics.push(quote! {
                    static #elem_static: ::aws_smithy_schema::Schema<'static> =
                        ::aws_smithy_schema::Schema::new_member(
                            ::aws_smithy_schema::shape_id!(#ns_lit, #list_shape_lit, "member"),
                            ::aws_smithy_schema::ShapeType::Structure,
                            "member",
                            0,
                        );
                });
                chain.extend(quote! { .with_list_member(&#elem_static) });
            }
            ValueKind::StringStringMap => {
                // Synthetic key/value members give the XML codec its element names.
                let key_static = format_ident!("MEMBER_{member_index}_KEY");
                let value_static = format_ident!("MEMBER_{member_index}_VALUE");
                let map_shape = format!(
                    "{shape_name}{}Map",
                    upper_camel_case(&field_ident.to_string())
                );
                let map_shape_lit = LitStr::new(&map_shape, field_ident.span());
                statics.push(quote! {
                    static #key_static: ::aws_smithy_schema::Schema<'static> =
                        ::aws_smithy_schema::Schema::new_member(
                            ::aws_smithy_schema::shape_id!(#ns_lit, #map_shape_lit, "key"),
                            ::aws_smithy_schema::ShapeType::String,
                            "key",
                            0,
                        );
                    static #value_static: ::aws_smithy_schema::Schema<'static> =
                        ::aws_smithy_schema::Schema::new_member(
                            ::aws_smithy_schema::shape_id!(#ns_lit, #map_shape_lit, "value"),
                            ::aws_smithy_schema::ShapeType::String,
                            "value",
                            1,
                        );
                });
                chain.extend(quote! { .with_map_members(&#key_static, &#value_static) });
            }
            _ => {}
        }

        // Arbitrary member traits go into a lazily built trait map.
        if !field_args.traits.is_empty() {
            let traits_static = format_ident!("MEMBER_{member_index}_TRAITS");
            let inserts = field_args.traits.iter().map(|expr| {
                quote! { map.insert(::std::boxed::Box::new(#expr)); }
            });
            statics.push(quote! {
                static #traits_static: ::std::sync::LazyLock<::aws_smithy_schema::TraitMap> =
                    ::std::sync::LazyLock::new(|| {
                        let mut map = ::aws_smithy_schema::TraitMap::new();
                        #(#inserts)*
                        map
                    });
            });
            chain.extend(quote! { .with_traits(&#traits_static) });
        }

        let shape_type = kind.shape_type();
        let member_index_lit = LitInt::new(&member_index.to_string(), field_ident.span());
        statics.push(quote! {
            static #member_static: ::aws_smithy_schema::Schema<'static> =
                ::aws_smithy_schema::Schema::new_member(
                    ::aws_smithy_schema::shape_id!(#ns_lit, #shape_name_lit, #member_name_lit),
                    #shape_type,
                    #member_name_lit,
                    #member_index_lit,
                )
                #chain;
        });
        member_refs.push(quote! { &#member_static });

        let write = kind.write_stmt(&member_static);
        serialize_stmts.push(if is_optional {
            quote! {
                if let Some(ref val) = self.#field_ident {
                    #write
                }
            }
        } else {
            quote! {
                {
                    let val = &self.#field_ident;
                    #write
                }
            }
        });

        member_index += 1;
    }

    // --- Struct schema ---
    let mut struct_chain = TokenStream2::new();
    if container.sensitive {
        struct_chain.extend(quote! { .with_sensitive() });
    }
    if let Some(v) = &container.xml_name {
        struct_chain.extend(quote! { .with_xml_name(#v) });
    }
    if container.error.is_some() || !container.traits.is_empty() {
        let error_insert = container.error.as_ref().map(|class| {
            quote! {
                map.insert(::std::boxed::Box::new(::aws_smithy_schema::StringTrait::new(
                    ::aws_smithy_schema::shape_id!("smithy.api", "error"),
                    #class,
                )));
            }
        });
        let inserts = container.traits.iter().map(|expr| {
            quote! { map.insert(::std::boxed::Box::new(#expr)); }
        });
        statics.push(quote! {
            static STRUCT_TRAITS: ::std::sync::LazyLock<::aws_smithy_schema::TraitMap> =
                ::std::sync::LazyLock::new(|| {
                    let mut map = ::aws_smithy_schema::TraitMap::new();
                    #error_insert
                    #(#inserts)*
                    map
                });
        });
        struct_chain.extend(quote! { .with_traits(&STRUCT_TRAITS) });
    }

    // --- Error shape impls ---
    let error_impls = if let Some(class) = &container.error {
        let status = match &container.http_error {
            Some(lit) => quote! { #lit },
            None if class.value() == "client" => quote! { 400 },
            None => quote! { 500 },
        };
        let display = if container.no_display {
            TokenStream2::new()
        } else {
            let message_field = fields
                .iter()
                .find(|f| f.ident.as_ref().is_some_and(|i| i == "message"));
            let body = match message_field {
                Some(f) => {
                    let (is_optional, _) = unwrap_option(&f.ty);
                    if is_optional {
                        quote! {
                            match &self.message {
                                Some(message) => f.write_str(message),
                                None => f.write_str(#shape_name_lit),
                            }
                        }
                    } else {
                        quote! { f.write_str(&self.message) }
                    }
                }
                None => quote! { f.write_str(#shape_name_lit) },
            };
            quote! {
                impl ::std::fmt::Display for #name {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        #body
                    }
                }
                impl ::std::error::Error for #name {}
            }
        };
        quote! {
            #display
            impl ::aws_smithy_http_server::schema::HttpModeledError for #name {
                fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
                    Self::SCHEMA
                }

                fn status_code(&self) -> u16 {
                    #status
                }
            }
        }
    } else {
        TokenStream2::new()
    };

    Ok(quote! {
        const _: () = {
            #(#statics)*

            static STRUCT_SCHEMA: ::aws_smithy_schema::Schema<'static> =
                ::aws_smithy_schema::Schema::new_struct(
                    ::aws_smithy_schema::shape_id!(#ns_lit, #shape_name_lit),
                    ::aws_smithy_schema::ShapeType::Structure,
                    &[#(#member_refs,)*],
                )
                #struct_chain;

            impl #name {
                /// The schema for this shape.
                pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &STRUCT_SCHEMA;
            }

            impl ::aws_smithy_schema::serde::SerializableStruct for #name {
                #[allow(unused_variables)]
                fn serialize_members(
                    &self,
                    ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
                ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
                    #(#serialize_stmts)*
                    Ok(())
                }
            }

            #error_impls
        };
    })
}

/// `flavor_text_entries` → `FlavorTextEntries`.
fn upper_camel_case(snake: &str) -> String {
    snake
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}
