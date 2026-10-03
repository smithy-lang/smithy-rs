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
mod descriptors;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::Parse;
use syn::{
    parse_macro_input, Data, DeriveInput, Expr, Fields, GenericArgument, Ident, LitInt, LitStr,
    PathArguments, Token, Type,
};

/// Derives a Smithy schema and optional serialization for a struct or single-field-variant enum.
///
/// Generates, for the annotated struct:
///
/// - A `Schema` static for the struct and one for each (non-skipped) field, exposed as the
///   associated constant `Self::SCHEMA` — the same convention used by smithy-rs generated code,
///   so hand-written and generated shapes nest freely in either direction.
/// - An `aws_smithy_schema::serde::SerializableStruct` implementation.
/// - For `@error` shapes: `Display` and `std::error::Error` implementations.
/// - For errors with `target = "server"` and serialization enabled: `HttpModeledError`.
///   Only this last implementation requires an `aws-smithy-http-server` dependency.
///
/// # Container attributes
///
/// - `#[smithy(serialize = false)]` — generates metadata without `SerializableStruct`
///   or `HttpModeledError`. Serialization defaults to `true`.
/// - `#[smithy(target = "shared" | "client" | "server")]` — selects runtime-specific
///   implementations. Defaults to `shared`; `client` also emits no server dependencies.
///   This is independent of `error = "client" | "server"`, which classifies the fault.
/// - `#[smithy(http = expr)]` — a const `aws_smithy_schema::traits::HttpTrait` expression.
/// - `#[smithy(original_name = "...")]`, `#[smithy(no_body_members)]`,
///   `#[smithy(streaming)]` — operation naming/body metadata and streaming union metadata.
///
/// - `#[smithy(namespace = "com.example")]` — the Smithy namespace of the shape. Required
///   unless supplied by [`smithy_namespace!`].
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
/// - `#[smithy(event_header)]`, `#[smithy(event_payload)]` — event member metadata.
/// - `#[smithy(streaming)]` — for `Receiver`, `EventStreamSender`, or `ByteStream` fields.
///   Keeps the member schema but omits the stream handle from `serialize_members`.
///   No marshalling or unmarshalling adapters are generated.
/// - `#[smithy(string_enum)]` — represents a named type as a string using its `as_str()`.
/// - `#[smithy(union)]` — represents a named type as a Smithy union instead of a structure.
///   Both annotations also apply to named elements/values inside collections, including
///   nested and sparse collections. Rust type paths alone do not identify modeled enums.
/// - `#[smithy(list_shape = "namespace#Name")]` — the modeled list's identity for element
///   metadata, instead of a synthetic name; supports the same `Vec` types as serialization.
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
/// `BigInteger`, `BigDecimal`, nested structures/unions, `Box<T>`, `Vec<T>`, and
/// `HashMap<String, T>`. Collections can nest and contain any supported value type.
/// `Option<T>` fields are omitted when `None`; `Option<T>` list elements and map values
/// represent sparse collections and serialize `None` as an explicit null.
/// `Vec<u8>` is rejected: use `Blob` for Smithy binary data.
/// Named types default to structures; use `union` or `string_enum` to override their kind.
/// Structures and unions must implement `SerializableStruct` when serialization is enabled.
/// Metadata-only derives impose no serialization bounds on their fields.
/// Generic struct/enum declarations (such as `Wrapper<T>`) are not supported.
///
/// # Example
///
/// ```ignore
/// use aws_smithy_schema::SmithySchema;
///
/// #[derive(Debug, SmithySchema)]
/// #[smithy(namespace = "pokemon_service.authz", target = "server", error = "client", http_error = 401)]
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

/// Derives an operation descriptor on a unit struct.
///
/// Requires `#[smithy(namespace = "...", input = Input, output = Output)]`;
/// optional `shape_name = "..."` and `errors(ErrorA, ErrorB)` customize the descriptor.
/// The namespace can be supplied by [`smithy_namespace!`]. Generates `Self::SCHEMA`
/// referring to an `aws_smithy_http_server::schema::OperationSchema`.
/// These descriptor derives currently support only the server runtime; explicit client/shared
/// targets are rejected. Both runtime crates must be direct dependencies.
#[proc_macro_derive(SmithyOperation, attributes(smithy))]
pub fn derive_smithy_operation(input: TokenStream) -> TokenStream {
    descriptors::descriptor(parse_macro_input!(input as DeriveInput), false)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derives a service descriptor on a unit struct.
///
/// Accepts `#[smithy(namespace = "...", version = "...", protocols("namespace#Protocol"),
/// operations(OperationA, OperationB))]`. Only `namespace` is required; `shape_name`
/// optionally overrides the struct name. List order is preserved. Generates `Self::SCHEMA`
/// referring to an `aws_smithy_http_server::schema::ServiceSchema`.
/// These descriptor derives currently support only the server runtime; explicit client/shared
/// targets are rejected. Both runtime crates must be direct dependencies.
#[proc_macro_derive(SmithyService, attributes(smithy))]
pub fn derive_smithy_service(input: TokenStream) -> TokenStream {
    descriptors::descriptor(parse_macro_input!(input as DeriveInput), true)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Delegates serialization, schema selection, HTTP status, Display, and Error::source
/// to the contained modeled error for each single-field enum variant.
/// Requires direct dependencies on `aws-smithy-schema` and `aws-smithy-http-server`.
/// Does not create a new Smithy shape ID: each variant retains its error's schema.
/// Accepts `#[smithy(target = "...", serialize = false)]`, including section defaults.
/// Defaults to shared serialization; only server target with serialization enabled
/// emits `HttpModeledError`. Display and Error delegation remain enabled.
#[proc_macro_derive(SmithyError, attributes(smithy))]
pub fn derive_smithy_error(input: TokenStream) -> TokenStream {
    descriptors::error(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Supplies a default Smithy namespace for a section of hand-written shapes.
///
/// Accepts a string literal, optional `target = "..."` and `serialize = true/false`
/// defaults after commas, a semicolon, and Rust items. For example:
/// `smithy_namespace! { "example", target = "server", serialize = false; /* items */ }`.
/// Directly enclosed types deriving `SmithySchema`, `SmithyOperation`, or `SmithyService`
/// receive the namespace unless explicitly set. `SmithySchema` and `SmithyError` inherit
/// target/serialization defaults; descriptors inherit the target. Per-type options win.
/// Items remain in the enclosing Rust scope, with their attributes and visibility preserved.
/// Qualified derive paths are supported; renamed derive imports and derives inside
/// `cfg_attr` are not recognized. Modules, function bodies, and macro invocations are
/// passed through without traversing their contents.
///
/// ```ignore
/// use aws_smithy_schema::{smithy_namespace, SmithySchema};
///
/// smithy_namespace! {
///     "smithy.example";
///
///     #[derive(Debug, SmithySchema)]
///     struct Nested {
///         name: String,
///     }
///
///     #[derive(Debug, SmithySchema)]
///     #[smithy(shape_name = "Renamed")]
///     struct Everything {
///         nested: Nested,
///     }
/// }
/// ```
#[proc_macro]
pub fn smithy_namespace(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as NamespaceItems);
    expand_namespace(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

struct NamespaceItems {
    namespace: LitStr,
    target: Option<LitStr>,
    serialize: Option<syn::LitBool>,
    items: Vec<syn::Item>,
}

impl Parse for NamespaceItems {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let namespace = input.parse()?;
        let mut target = None;
        let mut serialize = None;
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            let option: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if option == "target" && target.is_none() {
                let value: LitStr = input.parse()?;
                Target::parse(&value)?;
                target = Some(value);
            } else if option == "serialize" && serialize.is_none() {
                serialize = Some(input.parse()?);
            } else {
                return Err(syn::Error::new_spanned(
                    option,
                    "expected target or serialize, each at most once",
                ));
            }
        }
        input.parse::<Token![;]>()?;
        let mut items = Vec::new();
        while !input.is_empty() {
            items.push(input.parse()?);
        }
        Ok(Self {
            namespace,
            target,
            serialize,
            items,
        })
    }
}

fn expand_namespace(input: NamespaceItems) -> syn::Result<TokenStream2> {
    let NamespaceItems {
        namespace,
        target,
        serialize,
        mut items,
    } = input;
    for item in &mut items {
        let attrs = match item {
            syn::Item::Struct(item) => &mut item.attrs,
            syn::Item::Enum(item) => &mut item.attrs,
            _ => continue,
        };
        let mut derives_schema = false;
        let mut derives_error = false;
        let mut derives_descriptor = false;
        for attr in attrs.iter() {
            if attr.path().is_ident("derive") {
                let paths = attr.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, Token![,]>::parse_terminated,
                )?;
                for path in &paths {
                    if let Some(segment) = path.segments.last() {
                        match segment.ident.to_string().as_str() {
                            "SmithySchema" => derives_schema = true,
                            "SmithyError" => derives_error = true,
                            "SmithyOperation" | "SmithyService" => derives_descriptor = true,
                            _ => {}
                        }
                    }
                }
            }
        }
        if !derives_schema && !derives_error && !derives_descriptor {
            continue;
        }
        let mut has_namespace = false;
        let mut has_target = false;
        let mut has_serialize = false;
        for attr in attrs.iter() {
            if attr.path().is_ident("smithy") {
                let args = attr.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, Token![,]>::parse_terminated,
                )?;
                has_namespace |= args.iter().any(|arg| arg.path().is_ident("namespace"));
                has_target |= args.iter().any(|arg| arg.path().is_ident("target"));
                has_serialize |= args.iter().any(|arg| arg.path().is_ident("serialize"));
            }
        }
        if !has_namespace && (derives_schema || derives_descriptor) {
            attrs.push(syn::parse_quote!(#[smithy(namespace = #namespace)]));
        }
        if !has_target {
            if let Some(target) = &target {
                attrs.push(syn::parse_quote!(#[smithy(target = #target)]));
            }
        }
        if !has_serialize && (derives_schema || derives_error) {
            if let Some(serialize) = &serialize {
                attrs.push(syn::parse_quote!(#[smithy(serialize = #serialize)]));
            }
        }
    }
    Ok(quote!(#(#items)*))
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
    http: Option<Expr>,
    original_name: Option<LitStr>,
    no_body_members: bool,
    streaming: bool,
    serialize: Option<syn::LitBool>,
    target: Target,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Target {
    #[default]
    Shared,
    Client,
    Server,
}

impl Target {
    pub(crate) fn parse(value: &LitStr) -> syn::Result<Self> {
        match value.value().as_str() {
            "shared" => Ok(Self::Shared),
            "client" => Ok(Self::Client),
            "server" => Ok(Self::Server),
            _ => Err(syn::Error::new_spanned(
                value,
                "target must be shared, client, or server",
            )),
        }
    }
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
    event_header: bool,
    event_payload: bool,
    streaming: bool,
    string_enum: bool,
    union: bool,
    list_shape: Option<LitStr>,
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
            } else if meta.path.is_ident("http") {
                args.http = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("original_name") {
                args.original_name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("no_body_members") {
                args.no_body_members = true;
            } else if meta.path.is_ident("streaming") {
                args.streaming = true;
            } else if meta.path.is_ident("serialize") {
                args.serialize = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("target") {
                args.target = Target::parse(&meta.value()?.parse::<LitStr>()?)?;
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
                     `error`, `http_error`, `no_display`, `sensitive`, `xml_name`, `traits(...)`, `target`, `serialize`, `http`, `original_name`, `no_body_members`, `streaming`",
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
            } else if meta.path.is_ident("event_header") {
                args.event_header = true;
            } else if meta.path.is_ident("event_payload") {
                args.event_payload = true;
            } else if meta.path.is_ident("streaming") {
                args.streaming = true;
            } else if meta.path.is_ident("string_enum") {
                args.string_enum = true;
            } else if meta.path.is_ident("union") {
                args.union = true;
            } else if meta.path.is_ident("list_shape") {
                args.list_shape = Some(meta.value()?.parse()?);
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
                     `media_type`, `timestamp_format`, `traits(...)`, `streaming`, `event_header`, `event_payload`, `string_enum`, `union`, `list_shape`",
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
    List(Box<ValueKind>),
    Map(Box<ValueKind>),
    Optional(Box<ValueKind>),
    Boxed(Box<ValueKind>),
    Struct,
    Union,
    StringEnum,
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
            if path_segment(elem).is_some_and(|(ident, _)| ident == "u8") {
                return Err(unsupported(
                    "`Vec<u8>` — use `aws_smithy_types::Blob` for binary data",
                ));
            }
            ValueKind::List(Box::new(classify(elem, field)?))
        }
        "HashMap" => match args.as_slice() {
            [key, value]
                if path_segment(key)
                    .is_some_and(|(ident, args)| ident == "String" && args.is_empty()) =>
            {
                ValueKind::Map(Box::new(classify(value, field)?))
            }
            _ => return Err(unsupported("Smithy maps require String keys")),
        },
        "Option" | "Box" => {
            let [inner] = args.as_slice() else {
                return Err(unsupported("expected one type parameter"));
            };
            if ident == "Option" {
                if path_segment(inner).is_some_and(|(ident, _)| ident == "Option") {
                    return Err(unsupported("nested Option is not supported"));
                }
                ValueKind::Optional(Box::new(classify(inner, field)?))
            } else {
                ValueKind::Boxed(Box::new(classify(inner, field)?))
            }
        }
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
            ValueKind::List(_) => "List",
            ValueKind::Map(_) => "Map",
            ValueKind::Optional(inner) | ValueKind::Boxed(inner) => return inner.shape_type(),
            ValueKind::Struct => "Structure",
            ValueKind::Union => "Union",
            ValueKind::StringEnum => "String",
        };
        let ident = format_ident!("{variant}");
        quote! { ::aws_smithy_schema::ShapeType::#ident }
    }

    /// An attribute identifies the modeled kind of a named leaf type. A proc
    /// macro cannot resolve another Rust declaration from its field type path.
    fn annotate_named_type(&mut self, union: bool) -> Result<(), ()> {
        match self {
            Self::Struct => {
                *self = if union { Self::Union } else { Self::StringEnum };
                Ok(())
            }
            Self::List(inner) | Self::Map(inner) | Self::Optional(inner) | Self::Boxed(inner) => {
                inner.annotate_named_type(union)
            }
            _ => Err(()),
        }
    }

    fn collection_schema(
        &self,
        member: &Ident,
        namespace: &str,
        name: &str,
        modeled_id: Option<&LitStr>,
        statics: &mut Vec<TokenStream2>,
    ) -> syn::Result<TokenStream2> {
        let suffix = match self {
            Self::List(_) => "List",
            Self::Map(_) => "Map",
            Self::Optional(inner) | Self::Boxed(inner) => {
                return inner.collection_schema(member, namespace, name, modeled_id, statics)
            }
            _ => return Ok(TokenStream2::new()),
        };
        let (namespace, name) = if let Some(id) = modeled_id {
            let id_value = id.value();
            let Some((ns, shape)) = id_value.split_once('#') else {
                return Err(syn::Error::new_spanned(
                    id,
                    "list_shape must be a fully qualified shape ID: namespace#Name",
                ));
            };
            if ns.is_empty() || shape.is_empty() || shape.contains(['#', '$']) {
                return Err(syn::Error::new_spanned(
                    id,
                    "list_shape must identify a list shape, not a member",
                ));
            }
            (ns.to_owned(), shape.to_owned())
        } else {
            (namespace.to_owned(), format!("{name}{suffix}"))
        };
        let mut emit =
            |kind: &ValueKind, ident: &Ident, wire_name: &str, index: usize| -> syn::Result<()> {
                let nested_name = format!("{name}{}", upper_camel_case(wire_name));
                let chain =
                    kind.collection_schema(ident, &namespace, &nested_name, None, statics)?;
                let shape_type = kind.shape_type();
                statics.push(quote! {
                    static #ident: ::aws_smithy_schema::Schema<'static> =
                        ::aws_smithy_schema::Schema::new_member(
                            ::aws_smithy_schema::shape_id!(#namespace, #name, #wire_name),
                            #shape_type, #wire_name, #index,
                        ) #chain;
                });
                Ok(())
            };
        Ok(match self {
            Self::List(inner) => {
                let elem = format_ident!("{member}_ELEM");
                emit(inner, &elem, "member", 0)?;
                quote!(.with_list_member(&#elem))
            }
            Self::Map(inner) => {
                let key = format_ident!("{member}_KEY");
                let value = format_ident!("{member}_VALUE");
                emit(&Self::String, &key, "key", 0)?;
                emit(inner, &value, "value", 1)?;
                quote!(.with_map_members(&#key, &#value))
            }
            _ => unreachable!(),
        })
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
            ValueKind::List(inner) => {
                let elem = format_ident!("{member}_ELEM");
                let write = inner.write_stmt(&elem);
                // Keep bulk entry points for codecs that specialize primitive lists.
                match inner.as_ref() {
                    ValueKind::String => quote!(ser.write_string_list(&#member, val)?;),
                    ValueKind::Integer => quote!(ser.write_integer_list(&#member, val)?;),
                    ValueKind::Long => quote!(ser.write_long_list(&#member, val)?;),
                    ValueKind::Blob => quote!(ser.write_blob_list(&#member, val)?;),
                    _ => quote! {
                        ser.write_list(&#member, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                            for val in val { #write }
                            Ok(())
                        })?;
                    },
                }
            }
            ValueKind::Map(inner) => {
                let key = format_ident!("{member}_KEY");
                let value = format_ident!("{member}_VALUE");
                let write = inner.write_stmt(&value);
                quote! {
                    ser.write_map(&#member, &|ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer| {
                        for (key, val) in val {
                            ser.write_string(&#key, key)?;
                            #write
                        }
                        Ok(())
                    })?;
                }
            }
            ValueKind::Optional(inner) => {
                let write = inner.write_stmt(member);
                quote! {
                    if let Some(val) = val { #write } else { ser.write_null(&#member)?; }
                }
            }
            ValueKind::Boxed(inner) => {
                let write = inner.write_stmt(member);
                quote! { { let val = val.as_ref(); #write } }
            }
            ValueKind::StringEnum => quote!(ser.write_string(&#member, val.as_str())?;),
            ValueKind::Struct | ValueKind::Union => quote! { ser.write_struct(&#member, val)?; },
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
            "#[derive(SmithySchema)] requires a concrete struct or enum; generic declarations are not supported",
        ));
    }
    // A union variant is represented by a field with the variant's attributes/name.
    // This lets structs and unions share member metadata generation.
    let union_fields;
    let is_union = matches!(input.data, Data::Enum(_));
    let fields: Vec<&syn::Field> = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => named.named.iter().collect(),
            Fields::Unit => Vec::new(),
            Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    name,
                    "SmithySchema does not support tuple structs",
                ))
            }
        },
        Data::Enum(data) => {
            union_fields = data
                .variants
                .iter()
                .map(|variant| {
                    let Fields::Unnamed(fields) = &variant.fields else {
                        return Err(syn::Error::new_spanned(
                            variant,
                            "Smithy unions require one unnamed field per variant",
                        ));
                    };
                    if fields.unnamed.len() != 1 {
                        return Err(syn::Error::new_spanned(
                            variant,
                            "Smithy unions require one unnamed field per variant",
                        ));
                    }
                    let mut field = fields.unnamed[0].clone();
                    field.ident = Some(variant.ident.clone());
                    field.attrs.extend(variant.attrs.clone());
                    Ok(field)
                })
                .collect::<syn::Result<Vec<_>>>()?;
            union_fields.iter().collect()
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Rust unions are not supported",
            ))
        }
    };

    let container = parse_container_args(&input)?;
    let generate_serialization = container.serialize.as_ref().is_none_or(|value| value.value);
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
    if is_union && container.error.is_some() {
        return Err(syn::Error::new_spanned(
            name,
            "use SmithyError for an enum delegating to modeled errors",
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
            if is_union {
                return Err(syn::Error::new_spanned(
                    field,
                    "union variants cannot be skipped",
                ));
            }
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
        let mut kind = classify(value_ty, field)?;
        if field_args.string_enum && field_args.union {
            return Err(syn::Error::new_spanned(
                field,
                "string_enum and union cannot be combined",
            ));
        }
        if field_args.string_enum || field_args.union {
            kind.annotate_named_type(field_args.union).map_err(|()| {
                syn::Error::new_spanned(
                    field,
                    "string_enum and union require a named type, optionally inside collections",
                )
            })?;
        }
        if field_args.list_shape.is_some() && !matches!(kind, ValueKind::List(_)) {
            return Err(syn::Error::new_spanned(
                field,
                "list_shape requires a Vec field",
            ));
        }
        let streaming_type = path_segment(value_ty).map(|(ident, _)| ident.to_string());
        if field_args.streaming
            && !matches!(
                streaming_type.as_deref(),
                Some("Receiver" | "EventStreamSender" | "ByteStream")
            )
        {
            return Err(syn::Error::new_spanned(
                field,
                "streaming requires Receiver, EventStreamSender, or ByteStream",
            ));
        }
        if matches!(
            streaming_type.as_deref(),
            Some("Receiver" | "EventStreamSender" | "ByteStream")
        ) && !field_args.streaming
        {
            return Err(syn::Error::new_spanned(
                field,
                "stream fields require #[smithy(streaming)]",
            ));
        }
        if is_union && (field_args.skip || field_args.streaming || is_optional) {
            return Err(syn::Error::new_spanned(
                field,
                "union variants cannot be optional, skipped, or stream handles",
            ));
        }

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

        if field_args.event_header {
            chain.extend(quote!(.with_event_header()));
        }
        if field_args.event_payload {
            chain.extend(quote!(.with_event_payload()));
        }
        if field_args.streaming {
            chain.extend(quote!(.with_streaming()));
        }

        // Recursively attach collection member schemas at every nesting level.
        let collection_name = format!("{shape_name}{}", upper_camel_case(&field_ident.to_string()));
        chain.extend(kind.collection_schema(
            &member_static,
            &ns,
            &collection_name,
            field_args.list_shape.as_ref(),
            &mut statics,
        )?);

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

        let shape_type = if field_args.streaming {
            if streaming_type.as_deref() == Some("ByteStream") {
                quote!(::aws_smithy_schema::ShapeType::Blob)
            } else {
                quote!(::aws_smithy_schema::ShapeType::Union)
            }
        } else {
            kind.shape_type()
        };
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
        if field_args.streaming {
            // Stream bodies travel through the HTTP/event-stream adapters, not shape serde.
        } else if is_union {
            serialize_stmts.push(quote!(Self::#field_ident(val) => { #write }));
        } else {
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
        }
        member_index += 1;
    }

    let serialize_body = if is_union {
        quote!(match self { #(#serialize_stmts,)* })
    } else {
        quote!(#(#serialize_stmts)*)
    };
    let aggregate_type = if is_union {
        quote!(::aws_smithy_schema::ShapeType::Union)
    } else {
        quote!(::aws_smithy_schema::ShapeType::Structure)
    };

    // --- Struct schema ---
    let mut struct_chain = TokenStream2::new();
    if let Some(http) = &container.http {
        struct_chain.extend(quote!(.with_http(#http)));
    }
    if let Some(original) = &container.original_name {
        struct_chain.extend(quote!(.with_original_name(#original)));
    }
    if container.no_body_members {
        struct_chain.extend(quote!(.with_no_body_members()));
    }
    if container.streaming {
        struct_chain.extend(quote!(.with_streaming()));
    }
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
        let http_error =
            (generate_serialization && container.target == Target::Server).then(|| {
                quote! {
                    impl ::aws_smithy_http_server::schema::HttpModeledError for #name {
                        fn status_code(&self) -> u16 {
                            #status
                        }
                    }
                }
            });
        quote! {
            #display
            #http_error
        }
    } else {
        TokenStream2::new()
    };

    let serialization_impl = generate_serialization.then(|| {
        quote! {
                impl ::aws_smithy_schema::serde::SerializableStruct for #name {
                    #[allow(unused_variables)]
                    fn serialize_members(
                        &self,
                        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
                    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
                        #serialize_body
                        Ok(())
                    }
                }
        }
    });

    Ok(quote! {
        const _: () = {
            #(#statics)*

            static STRUCT_SCHEMA: ::aws_smithy_schema::Schema<'static> =
                ::aws_smithy_schema::Schema::new_struct(
                    ::aws_smithy_schema::shape_id!(#ns_lit, #shape_name_lit),
                    #aggregate_type,
                    &[#(#member_refs,)*],
                )
                #struct_chain;

            impl #name {
                /// The schema for this shape.
                pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &STRUCT_SCHEMA;
            }

            #serialization_impl

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

#[cfg(test)]
mod namespace_tests {
    use super::*;

    #[test]
    fn rejects_invalid_collection_and_enum_annotations() {
        for input in [
            syn::parse_quote!(
                #[smithy(namespace = "test")]
                struct Invalid {
                    values: HashMap<i32, String>,
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "test")]
                struct Invalid {
                    values: Vec<u8>,
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "test")]
                struct Invalid {
                    #[smithy(union)]
                    values: Vec<String>,
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "test")]
                struct Invalid {
                    #[smithy(union, string_enum)]
                    value: Choice,
                }
            ),
        ] {
            assert!(expand(input).is_err());
        }
    }

    #[test]
    fn rejects_invalid_wrapper_syntax() {
        for input in [
            quote!(),
            quote!(123; struct Shape;),
            quote!("example" struct Shape;),
            quote!("example"; let value = 1;),
        ] {
            assert!(syn::parse2::<NamespaceItems>(input).is_err());
        }
        let empty = syn::parse2::<NamespaceItems>(quote!("example";)).unwrap();
        assert!(expand_namespace(empty).unwrap().is_empty());
    }

    #[test]
    fn does_not_traverse_nested_items_or_macro_invocations() {
        let items = quote! {
            mod nested {
                #[derive(SmithySchema)]
                struct Shape;
            }
            fn local() {
                #[derive(SmithySchema)]
                struct Shape;
            }
            other_macro! {
                #[derive(SmithySchema)]
                struct Shape;
            }
        };
        let input = syn::parse2::<NamespaceItems>(quote!("example"; #items)).unwrap();
        assert_eq!(
            expand_namespace(input).unwrap().to_string(),
            items.to_string()
        );
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;

    #[test]
    fn rejects_incompatible_field_and_generation_options() {
        for input in [
            syn::parse_quote!(
                #[smithy(namespace = "example", target = "invalid")]
                struct Invalid;
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example", serialize = "false")]
                struct Invalid;
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example")]
                struct Invalid {
                    #[smithy(streaming)]
                    value: String,
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example")]
                struct Invalid {
                    value: ByteStream,
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example")]
                struct Invalid {
                    #[smithy(list_shape = "example#List")]
                    value: String,
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example")]
                struct Invalid {
                    #[smithy(list_shape = "List")]
                    value: Vec<String>,
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example")]
                enum Invalid {
                    #[smithy(skip)]
                    Value(String),
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example")]
                enum Invalid {
                    Value(Option<String>),
                }
            ),
            syn::parse_quote!(
                #[smithy(namespace = "example")]
                enum Invalid {
                    Value,
                }
            ),
        ] {
            assert!(expand(input).is_err());
        }
    }
}
