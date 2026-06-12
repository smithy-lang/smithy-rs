/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Basic test of using the mock_client macro from an "external" crate

mod fake_crate {
    pub(crate) mod error {
        #[derive(Debug)]
        pub(crate) struct BuildError {}
    }

    pub(crate) mod operation {
        pub(crate) mod do_something {
            #[derive(Debug)]
            pub(crate) struct DoSomethingInput {}

            #[derive(Debug)]
            pub(crate) struct DoSomethingOutput {}

            #[derive(Debug)]
            pub(crate) enum DoSomethingError {}
            impl std::fmt::Display for DoSomethingError {
                fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    todo!()
                }
            }
            impl std::error::Error for DoSomethingError {}

            pub(crate) mod builders {
                use aws_smithy_runtime_api::client::result::SdkError;

                #[derive(Clone)]
                pub(crate) struct DoSomethingInputBuilder {}
                impl DoSomethingInputBuilder {
                    pub(crate) fn build(
                        &self,
                    ) -> Result<super::DoSomethingInput, crate::fake_crate::error::BuildError>
                    {
                        todo!()
                    }
                }

                pub(crate) struct DoSomethingFluentBuilder {}
                impl DoSomethingFluentBuilder {
                    pub(crate) fn as_input(&self) -> &DoSomethingInputBuilder {
                        todo!()
                    }

                    pub(crate) async fn send(
                        self,
                    ) -> Result<super::DoSomethingOutput, SdkError<super::DoSomethingError, ()>>
                    {
                        todo!()
                    }
                }
            }
        }
    }

    pub(crate) mod client {
        use crate::fake_crate::config;

        pub(crate) struct Client {}
        impl Client {
            pub(crate) fn from_conf(_conf: config::Config) -> Self {
                Self {}
            }

            pub(crate) fn do_something(
                &self,
            ) -> super::operation::do_something::builders::DoSomethingFluentBuilder {
                super::operation::do_something::builders::DoSomethingFluentBuilder {}
            }
        }
    }

    pub(crate) mod config {
        use aws_smithy_runtime_api::client::http::SharedHttpClient;
        use aws_smithy_runtime_api::client::interceptors::Intercept;

        pub(crate) struct Config {}
        impl Config {
            pub(crate) fn builder() -> Builder {
                Builder {}
            }
        }
        pub(crate) struct Builder {}
        impl Builder {
            pub fn build(self) -> Config {
                Config {}
            }
            pub fn with_test_defaults_v2(self) -> Self {
                Self {}
            }
            pub fn http_client(self, _http_client: SharedHttpClient) -> Self {
                Self {}
            }

            pub fn interceptor(self, _interceptor: impl Intercept + 'static) -> Self {
                self
            }
        }
    }
}
#[test]
fn mock_client() {
    aws_smithy_mocks::mock_client!(fake_crate, &[]);
}

#[test]
fn mock_oneline() {
    let rule =
        aws_smithy_mocks::mock!(fake_crate::client::Client::do_something).then_output(|| todo!());
    let _ = aws_smithy_mocks::mock_client!(fake_crate, &[rule]);
}

#[test]
fn mock_let() {
    let rule_builder = aws_smithy_mocks::mock!(fake_crate::client::Client::do_something);
    let rule = rule_builder.then_output(|| todo!());
    let _ = aws_smithy_mocks::mock_client!(fake_crate, &[rule]);
}
