use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Attribute;
use syn::Ident;
use syn::ItemStruct;

use crate::SmithyMetricsStructAttrs;

pub(crate) fn smithy_metrics_impl(
    attrs: SmithyMetricsStructAttrs,
    mut metrics_struct: ItemStruct,
) -> TokenStream2 {
    let syn::Fields::Named(ref mut fields) = metrics_struct.fields else {
        return syn::Error::new_spanned(metrics_struct, "only named fields are supported")
            .to_compile_error();
    };

    // Remove smithy_metrics attributes from fields to avoid compilation errors
    for field in &mut fields.named {
        field.attrs = clean_attrs(&field.attrs);
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
    let ext_trait_impls = generate_ext_trait_impl(&attrs.server_crate, &metrics_struct.ident);

    quote! {
        #metrics_struct
        #ext_trait
        #ext_trait_impls
    }
}

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

fn generate_ext_trait_impl(_sdk_crate: &Ident, struct_ident: &Ident) -> TokenStream2 {
    let lowercase_struct_name = quote::format_ident!("{}", struct_ident.to_string().to_lowercase());
    let trait_name = quote::format_ident!("{}BuildExt", struct_ident);
    let macro_name = quote::format_ident!("impl_build_{}_for_state", lowercase_struct_name);
    
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
fn clean_attrs(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("smithy_metrics"))
        .cloned()
        .collect()
}
