/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Defines deterministic handlers shared by the compatibility services and mainline
//! recorder. Their outputs expose bindings, payloads, labels, and errors so the same
//! request scenario can be compared across generated implementations.
//!
//! The handler macro shared with the `protocol-compat-mainline` recorder.

macro_rules! compat_service {
    ($module:ident, $krate:ident, $service:ident, $config:ident, [$($builder_args:tt)*], $download:expr, $ret:ty) => {
        pub mod $module {
            use $krate::error::{DeleteWidgetError, GetWidgetError, GreedyLabelError, TrailingSlashLabelError};
            use $krate::input::*;
            use $krate::model::Inner;
            use $krate::output::*;
            pub use super::kit::Body;
            pub use $krate::{$config, $service};

            async fn get_widget(input: GetWidgetInput) -> Result<GetWidgetOutput, GetWidgetError> {
                Ok(GetWidgetOutput { id: Some(input.id) })
            }
            async fn delete_widget(_: DeleteWidgetInput) -> Result<DeleteWidgetOutput, DeleteWidgetError> {
                Ok(DeleteWidgetOutput {})
            }
            async fn ping(_: PingInput) -> PingOutput {
                PingOutput {
                    message: Some("pong".to_owned()),
                }
            }
            async fn response_code(_: ResponseCodeInput) -> ResponseCodeOutput {
                ResponseCodeOutput {
                    value: Some("v".to_owned()),
                }
            }
            async fn response_code_member(_: ResponseCodeMemberInput) -> ResponseCodeMemberOutput {
                ResponseCodeMemberOutput {
                    status: Some(202),
                    value: Some("v".to_owned()),
                }
            }
            async fn response_header(_: ResponseHeaderInput) -> ResponseHeaderOutput {
                ResponseHeaderOutput {
                    header: Some("h".to_owned()),
                    value: Some("v".to_owned()),
                }
            }
            async fn struct_payload(input: StructPayloadInput) -> StructPayloadOutput {
                StructPayloadOutput {
                    h: input.h,
                    payload: input.payload.map(|p: Inner| Inner { a: p.a }),
                }
            }
            async fn download_blob(_: DownloadBlobInput) -> DownloadBlobOutput {
                DownloadBlobOutput {
                    data: $download,
                }
            }
            async fn prefix_headers(input: PrefixHeadersInput) -> PrefixHeadersOutput {
                PrefixHeadersOutput { echoed: input.meta }
            }
            async fn media_type_map(input: MediaTypeMapInput) -> MediaTypeMapOutput {
                MediaTypeMapOutput {
                    echoed: input.values,
                }
            }
            async fn trailing_slash_label(input: TrailingSlashLabelInput) -> Result<TrailingSlashLabelOutput, TrailingSlashLabelError> {
                Ok(TrailingSlashLabelOutput {
                    name: Some(input.name),
                })
            }
            async fn greedy_label(input: GreedyLabelInput) -> Result<GreedyLabelOutput, GreedyLabelError> {
                Ok(GreedyLabelOutput {
                    key: Some(input.key),
                })
            }
            /// A value `serde_json` would never emit as a number: serializing it fails.
            async fn sensitive_amount(_: SensitiveAmountInput) -> SensitiveAmountOutput {
                SensitiveAmountOutput {
                    amount: Some("+000987654321".parse().unwrap()),
                }
            }

            /// A date-time far outside the range any formatter can render: serializing it fails.
            async fn sensitive_timestamp(_: SensitiveTimestampInput) -> SensitiveTimestampOutput {
                SensitiveTimestampOutput {
                    when: Some(aws_smithy_types::DateTime::from_secs(i64::MAX)),
                }
            }

            pub fn service() -> $ret {
                $service::builder$($builder_args)*($config::builder().build())
                    .get_widget(get_widget)
                    .delete_widget(delete_widget)
                    .ping(ping)
                    .response_code(response_code)
                    .response_code_member(response_code_member)
                    .response_header(response_header)
                    .struct_payload(struct_payload)
                    .download_blob(download_blob)
                    .prefix_headers(prefix_headers)
                    .media_type_map(media_type_map)
                    .trailing_slash_label(trailing_slash_label)
                    .greedy_label(greedy_label)
                    .sensitive_amount(sensitive_amount)
                    .sensitive_timestamp(sensitive_timestamp)
                    .build()
                    .expect("every operation has a handler")
            }
        }
    };
}

