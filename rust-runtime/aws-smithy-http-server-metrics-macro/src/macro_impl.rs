use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Attribute;
use syn::Ident;
use syn::ItemStruct;

use crate::SmithyMetricsStructAttrs;

/// Represents a field marked with `#[smithy_metrics(operation)]` that needs
/// to be inserted as an extension into HTTP requests.
struct ExtensionField {
    name: Ident,
    ty: syn::Type,
}

/// Implementation of the `#[smithy_metrics]` procedural macro.
///
/// This macro:
/// 1. Processes fields marked with `#[smithy_metrics(operation)]`
/// 2. Wraps their types in `metrique::Slot<T>` if not already wrapped
/// 3. Adds default request/repsonse metrics fields
/// 4. Generates a builder trait and implementations for the metrics layer for the annotated metrics struct
pub(crate) fn smithy_metrics_impl(
    attrs: SmithyMetricsStructAttrs,
    mut metrics_struct: ItemStruct,
) -> TokenStream2 {
    let syn::Fields::Named(ref mut fields) = metrics_struct.fields else {
        return syn::Error::new_spanned(metrics_struct, "only named fields are supported")
            .to_compile_error();
    };

    let mut extension_fields = Vec::new();

    // Collect extension fields and remove smithy_metrics attributes
    for field in &mut fields.named {
        let has_operation = has_operation_attr(&field.attrs);
        field.attrs = clean_attrs(&field.attrs);

        if !has_operation {
            continue;
        }

        let Some(field_name) = field.ident.clone() else {
            return syn::Error::new_spanned(field, "extension field must have a name")
                .to_compile_error();
        };

        // Wrap the field type in metrique::Slot<...> if not already wrapped in Slot
        if let Some(unwrapped_ty) = extract_inner_type(&field.ty) {
            // Already wrapped in Slot, keep as-is
            extension_fields.push(ExtensionField {
                name: field_name,
                ty: unwrapped_ty,
            });
        } else {
            // Not wrapped, wrap it
            let ty = field.ty.clone();
            field.ty = syn::parse_quote! {
                metrique::Slot<#ty>
            };
            extension_fields.push(ExtensionField {
                name: field_name,
                ty,
            });
        }
    }

    fields.named.push(syn::parse_quote! {
        #[metrics(flatten)]
        default_request_metrics: Option<metrique::Slot<aws_smithy_http_server_metrics::default::DefaultRequestMetrics>>
    });

    fields.named.push(syn::parse_quote! {
        #[metrics(flatten)]
        default_response_metrics: Option<metrique::Slot<aws_smithy_http_server_metrics::default::DefaultResponseMetrics>>
    });

    let ext_trait = generate_ext_trait(&attrs.server_crate, &metrics_struct.ident);
    let ext_trait_impls = generate_ext_trait_impl(
        &attrs.server_crate,
        &metrics_struct.ident,
        &extension_fields,
    );

    quote! {
        #metrics_struct
        #ext_trait
        #ext_trait_impls
    }
}

/// Generates a builder extension trait for the metrics struct.
fn generate_ext_trait(_sdk_crate: &Ident, metrics_struct: &Ident) -> TokenStream2 {
    let trait_name = quote::format_ident!("{}BuildExt", metrics_struct);
    quote! {
        pub trait #trait_name<S, I, Rq, Rs>
        where
            S: aws_smithy_http_server_metrics::traits::MetriqueEntrySink<#metrics_struct>,
            I: aws_smithy_http_server_metrics::traits::InitMetrics<#metrics_struct, S>,
            Rq: aws_smithy_http_server_metrics::traits::SetRequestMetrics<#metrics_struct, S>,
            Rs: aws_smithy_http_server_metrics::traits::SetResponseMetrics<#metrics_struct, S>,
        {
            fn build(self) -> aws_smithy_http_server_metrics::layer::MetricsLayer<#metrics_struct, S, I, Rq, Rs>;
        }
    }
}

fn generate_ext_trait_impl(
    _sdk_crate: &Ident,
    struct_ident: &Ident,
    extension_fields: &[ExtensionField],
) -> TokenStream2 {
    let lowercase_struct_name = quote::format_ident!("{}", struct_ident.to_string().to_lowercase());
    let trait_name = quote::format_ident!("{}BuildExt", struct_ident);
    let macro_name = quote::format_ident!("impl_build_{}_for_state", lowercase_struct_name);

    let extension_insertions = extension_fields.iter().map(|ext| {
        let field_name = &ext.name;
        let ty = &ext.ty;
        quote! {
            metrics.#field_name = metrique::Slot::new(<#ty>::default());
            let extension_slotguard = metrics
                .#field_name
                .open(metrique::OnParentDrop::Discard)
                .expect("unreachable: the slot was created in this scope and is not opened before this point");
            req.extensions_mut().insert(extension_slotguard);
        }
    });

    quote! {
        macro_rules! #macro_name {
            ($state:ty) => {
                impl<S, I, Rq, Rs> #trait_name<S, I, Rq, Rs> for aws_smithy_http_server_metrics::MetricsLayerBuilder<$state, #struct_ident, S, I, Rq, Rs>
                where
                    S: aws_smithy_http_server_metrics::traits::MetriqueEntrySink<#struct_ident>,
                    I: aws_smithy_http_server_metrics::traits::InitMetrics<#struct_ident, S>,
                    Rq: aws_smithy_http_server_metrics::traits::SetRequestMetrics<#struct_ident, S>,
                    Rs: aws_smithy_http_server_metrics::traits::SetResponseMetrics<#struct_ident, S>,
                {
                    fn build(self) -> aws_smithy_http_server_metrics::layer::MetricsLayer<#struct_ident, S, I, Rq, Rs> {
                        let default_req_metrics_extension_fn =
                            |req: &mut http::Request<aws_smithy_http_server_metrics::types::ReqBody>,
                            metrics: &mut metrique::AppendAndCloseOnDrop<#struct_ident, S>,
                            config: aws_smithy_http_server_metrics::default::DefaultRequestMetricsConfig| {
                                metrics.default_request_metrics = Some(metrique::Slot::new(aws_smithy_http_server_metrics::default::DefaultRequestMetrics::default()));
                                let default_req_metrics_slotguard = metrics
                                    .default_request_metrics
                                    .as_mut()
                                    .expect("unreachable: the option is set to some in this scope")
                                    .open(metrique::OnParentDrop::Discard)
                                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                                let ext = aws_smithy_http_server_metrics::default::DefaultRequestMetricsExtension::__macro_new(
                                    default_req_metrics_slotguard,
                                    config,
                                );

                                req.extensions_mut().insert(ext);

                                #(#extension_insertions)*
                            };

                        let default_res_metrics_extension_fn =
                            |res: &mut http::Response<aws_smithy_http_server_metrics::types::ResBody>,
                            metrics: &mut metrique::AppendAndCloseOnDrop<#struct_ident, S>,
                            config: aws_smithy_http_server_metrics::default::DefaultResponseMetricsConfig| {
                                metrics.default_response_metrics = Some(metrique::Slot::new(aws_smithy_http_server_metrics::default::DefaultResponseMetrics::default()));
                                let default_res_metrics_slotguard = metrics
                                    .default_response_metrics
                                    .as_mut()
                                    .expect("unreachable: the option is set to some in this scope")
                                    .open(metrique::OnParentDrop::Discard)
                                    .expect("unreachable: the slot was created in this scope and is not opened before this point");

                                let ext = aws_smithy_http_server_metrics::default::DefaultResponseMetricsExtension::__macro_new(
                                    default_res_metrics_slotguard,
                                    config,
                                );

                                res.extensions_mut().insert(ext);
                            };

                        aws_smithy_http_server_metrics::layer::MetricsLayer::__macro_new(
                            self.init_metrics.expect("init_metrics must be provided"),
                            self.set_request_metrics,
                            self.set_response_metrics,
                            default_req_metrics_extension_fn,
                            default_res_metrics_extension_fn,
                            self.default_req_metrics_config,
                            self.default_res_metrics_config,
                        )
                    }
                }
            };
        }

        #macro_name!(aws_smithy_http_server_metrics::layer::builder::WithDefaults);
        #macro_name!(aws_smithy_http_server_metrics::layer::builder::WithRq);
        #macro_name!(aws_smithy_http_server_metrics::layer::builder::WithRs);
        #macro_name!(aws_smithy_http_server_metrics::layer::builder::WithRqAndRs);
    }
}

/// Removes all `#[smithy_metrics(...)]` attributes from a field.
///
/// These attibutes are only used by the macro for processing and are not
/// valid Rust attributes, so they must be removed before code generation.
fn clean_attrs(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("smithy_metrics"))
        .cloned()
        .collect()
}

/// Checks if a field has the `#[smithy_metrics(operation)]` attribute.
fn has_operation_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        // Check if attribute path is "smithy_metrics"
        if !attr.path().is_ident("smithy_metrics") {
            return false;
        }
        
        // Parse the attribute arguments to check for "operation"
        attr.parse_args::<syn::Ident>()
            .map(|ident| ident == "operation")
            .unwrap_or(false)
    })
}

/// Recursively searches a type for `Slot<T>` and extracts the inner type `T`, otherwise returns None.
/// 
/// Examples:
/// - `MyType` → `None`
/// - `Slot<MyType>` → `Some(MyType)`
/// - `Option<Slot<MyType>>` → `Some(MyType)`
/// - `Vec<Option<Slot<MyType>>>` → `Some(MyType)`
fn extract_inner_type(ty: &syn::Type) -> Option<syn::Type> {
    // Only handle path types (e.g., Foo::Bar<T>)
    let syn::Type::Path(type_path) = ty else {
        return None;
    };

    // Iterate through each segment of the path (e.g., std::option::Option has 3 segments)
    for segment in &type_path.path.segments {
        // Check if this segment is "Slot"
        if segment.ident == "Slot" {
            // Extract the generic argument: Slot<T> → T
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                continue;
            };
            let syn::GenericArgument::Type(inner_ty) = args.args.first()? else {
                continue;
            };
            return Some(inner_ty.clone());
        }

        // If not "Slot", recursively search generic arguments
        // This handles cases like Option<Slot<T>> or Vec<Slot<T>>
        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
            for arg in &args.args {
                if let syn::GenericArgument::Type(inner_ty) = arg {
                    if let Some(found) = extract_inner_type(inner_ty) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to extract the generated struct from macro output
    fn get_generated_struct(output: TokenStream2) -> syn::ItemStruct {
        let file: syn::File = syn::parse2(output).expect("should parse generated code");
        file.items.iter().find_map(|item| {
            if let syn::Item::Struct(s) = item {
                Some(s.clone())
            } else {
                None
            }
        }).expect("should have generated struct")
    }

    /// Helper to find a field by name in a struct
    fn find_field<'a>(struct_item: &'a syn::ItemStruct, field_name: &str) -> &'a syn::Field {
        let syn::Fields::Named(fields) = &struct_item.fields else {
            panic!("should have named fields");
        };
        fields.named.iter()
            .find(|f| f.ident.as_ref().unwrap() == field_name)
            .expect(&format!("should have field {}", field_name))
    }

    /// Helper to assert that a field has the expected type
    fn assert_field_type(struct_item: &syn::ItemStruct, field_name: &str, expected_ty: syn::Type) {
        let field = find_field(struct_item, field_name);
        let actual_ty = &field.ty;
        assert_eq!(
            quote::quote!(#actual_ty).to_string(),
            quote::quote!(#expected_ty).to_string(),
            "field '{}' should have type {}", 
            field_name,
            quote::quote!(#expected_ty)
        );
    }

    #[test]
    fn test_wraps_operation_field_in_slot() {
        let input: ItemStruct = syn::parse_quote! {
            struct MyMetrics {
                #[smithy_metrics(operation)]
                my_field: MyType,
            }
        };
        let attrs = SmithyMetricsStructAttrs {
            server_crate: syn::parse_quote!(my_crate),
        };

        let output = smithy_metrics_impl(attrs, input);
        let generated_struct = get_generated_struct(output);
        
        // Verify my_field is wrapped in Slot
        assert_field_type(&generated_struct, "my_field", syn::parse_quote!(metrique::Slot<MyType>));
        
        // Verify default fields exist
        find_field(&generated_struct, "default_request_metrics");
        find_field(&generated_struct, "default_response_metrics");
    }

    #[test]
    fn test_already_wrapped_slot_not_double_wrapped() {
        let input: ItemStruct = syn::parse_quote! {
            struct MyMetrics {
                #[smithy_metrics(operation)]
                my_field: Slot<MyType>,
            }
        };
        let attrs = SmithyMetricsStructAttrs {
            server_crate: syn::parse_quote!(my_crate),
        };

        let output = smithy_metrics_impl(attrs, input);
        let generated_struct = get_generated_struct(output);
        
        // Verify my_field is NOT double-wrapped
        assert_field_type(&generated_struct, "my_field", syn::parse_quote!(Slot<MyType>));
    }

    #[test]
    fn test_option_slot_not_double_wrapped() {
        let input: ItemStruct = syn::parse_quote! {
            struct MyMetrics {
                #[smithy_metrics(operation)]
                my_field: Option<Slot<MyType>>,
            }
        };
        let attrs = SmithyMetricsStructAttrs {
            server_crate: syn::parse_quote!(my_crate),
        };

        let output = smithy_metrics_impl(attrs, input);
        let generated_struct = get_generated_struct(output);
        
        // Verify my_field is NOT double-wrapped
        assert_field_type(&generated_struct, "my_field", syn::parse_quote!(Option<Slot<MyType>>));
    }

    #[test]
    fn test_mixed_operation_and_regular_fields() {
        let input: ItemStruct = syn::parse_quote! {
            struct MyMetrics {
                #[smithy_metrics(operation)]
                operation_field: MyType,
                regular_field: String,
                another_regular: i32,
            }
        };
        let attrs = SmithyMetricsStructAttrs {
            server_crate: syn::parse_quote!(my_crate),
        };

        let output = smithy_metrics_impl(attrs, input);
        let generated_struct = get_generated_struct(output);
        
        // Verify operation_field is wrapped
        assert_field_type(&generated_struct, "operation_field", syn::parse_quote!(metrique::Slot<MyType>));
        
        // Verify regular fields are unchanged
        assert_field_type(&generated_struct, "regular_field", syn::parse_quote!(String));
        assert_field_type(&generated_struct, "another_regular", syn::parse_quote!(i32));
    }

    #[test]
    fn test_multiple_operation_fields() {
        let input: ItemStruct = syn::parse_quote! {
            struct MyMetrics {
                #[smithy_metrics(operation)]
                first_operation: FirstType,
                #[smithy_metrics(operation)]
                second_operation: SecondType,
                #[smithy_metrics(operation)]
                third_operation: ThirdType,
            }
        };
        let attrs = SmithyMetricsStructAttrs {
            server_crate: syn::parse_quote!(my_crate),
        };

        let output = smithy_metrics_impl(attrs, input);
        let generated_struct = get_generated_struct(output);
        
        // Verify all operation fields are wrapped
        assert_field_type(&generated_struct, "first_operation", syn::parse_quote!(metrique::Slot<FirstType>));
        assert_field_type(&generated_struct, "second_operation", syn::parse_quote!(metrique::Slot<SecondType>));
        assert_field_type(&generated_struct, "third_operation", syn::parse_quote!(metrique::Slot<ThirdType>));
    }

    // A few more complex tets for extract_inner_type

    #[test]
    fn test_extract_inner_type_nested() {
        let ty: syn::Type = syn::parse_quote!(Vec<Option<Slot<MyType>>>);
        let result = extract_inner_type(&ty).expect("should extract inner type");
        let expected: syn::Type = syn::parse_quote!(MyType);
        assert_eq!(quote::quote!(#result).to_string(), quote::quote!(#expected).to_string());
    }

    #[test]
    fn test_extract_inner_type_complex_inner() {
        let ty: syn::Type = syn::parse_quote!(Slot<std::collections::HashMap<String, i32>>);
        let result = extract_inner_type(&ty).expect("should extract inner type");
        let expected: syn::Type = syn::parse_quote!(std::collections::HashMap<String, i32>);
        assert_eq!(quote::quote!(#result).to_string(), quote::quote!(#expected).to_string());
    }
}
