/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization

/**
 * Add a `make_token` field to Service config. See below for the resulting generated code.
 */
class IdempotencyTokenProviderCustomization : NamedCustomization<ServiceConfig>() {
    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigStruct -> writable {
                rust("pub (crate) make_token: #T::IdempotencyTokenProvider,", RuntimeType.IdempotencyToken)
            }

            ServiceConfig.ConfigImpl -> writable {
                rust(
                    """
                    /// Returns a copy of the idempotency token provider.
                    /// If a random token provider was configured,
                    /// a newly-randomized token provider will be returned.
                    pub fn make_token(&self) -> #T::IdempotencyTokenProvider {
                        self.make_token.clone()
                    }
                    """,
                    RuntimeType.IdempotencyToken,
                )
            }

            ServiceConfig.BuilderStruct -> writable {
                rust("make_token: Option<#T::IdempotencyTokenProvider>,", RuntimeType.IdempotencyToken)
            }

            ServiceConfig.BuilderImpl -> writable {
                rustTemplate(
                    """
                    /// Sets the idempotency token provider to use for service calls that require tokens.
                    pub fn make_token(mut self, make_token: impl Into<#{TokenProvider}>) -> Self {
                        self.set_make_token(Some(make_token.into()));
                        self
                    }

                    /// Sets the idempotency token provider to use for service calls that require tokens.
                    pub fn set_make_token(&mut self, make_token: Option<#{TokenProvider}>) -> &mut Self {
                        self.make_token = make_token;
                        self
                    }
                    """,
                    "TokenProvider" to RuntimeType.IdempotencyToken.resolve("IdempotencyTokenProvider"),
                )
            }

            ServiceConfig.BuilderBuild -> writable {
                rust("make_token: self.make_token.unwrap_or_else(#T::default_provider),", RuntimeType.IdempotencyToken)
            }

            is ServiceConfig.DefaultForTests -> writable { rust("""${section.configBuilderRef}.set_make_token(Some("00000000-0000-4000-8000-000000000000".into()));""") }

            else -> writable { }
        }
    }
}

/* Generated Code
pub struct Config {
    pub(crate) make_token: Box<dyn crate::idempotency_token::MakeIdempotencyToken>,
}
impl Config {
    pub fn builder() -> Builder {
        Builder::default()
    }
}
#[derive(Default)]
pub struct Builder {
    #[allow(dead_code)]
    make_token: Option<Box<dyn crate::idempotency_token::MakeIdempotencyToken>>,
}
impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the idempotency token provider to use for service calls that require tokens.
    pub fn make_token(
        mut self,
        make_token: impl crate::idempotency_token::MakeIdempotencyToken + 'static,
    ) -> Self {
        self.make_token = Some(Box::new(make_token));
        self
    }

    pub fn build(self) -> Config {
        Config {
            make_token: self
            .make_token
            .unwrap_or_else(|| Box::new(crate::idempotency_token::default_provider())),
        }
    }
}
 */
