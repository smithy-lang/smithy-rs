/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.config

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.NamedSectionGenerator

/**
 * Add a `token_provider` field to Service config. See below for the resulting generated code.
 */
class IdempotencyProviderConfig : NamedSectionGenerator<ServiceConfig>() {
    override fun section(section: ServiceConfig): Writable = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust(
                "pub (crate) token_provider: Box<dyn #T::ProvideIdempotencyToken>,",
                RuntimeType.IdempotencyToken
            )
            is ServiceConfig.ConfigImpl -> {
            }
            is ServiceConfig.BuilderStruct -> rust(
                "token_provider: Option<Box<dyn #T::ProvideIdempotencyToken>>,",
                RuntimeType.IdempotencyToken
            )
            is ServiceConfig.BuilderImpl -> rust(
                """
            pub fn token_provider(mut self, token_provider: impl #T::ProvideIdempotencyToken + 'static) -> Self {
                self.token_provider = Some(Box::new(token_provider));
                self
            }
            """,
                RuntimeType.IdempotencyToken
            )
            is ServiceConfig.BuilderBuild ->
                rust(
                    "token_provider: self.token_provider.unwrap_or_else(|| Box::new(#T::default_provider())),",
                    RuntimeType.IdempotencyToken
                )
        }
    }
}

/* Generated Code
pub struct Config {
    pub(crate) token_provider: Box<dyn crate::idempotency_token::ProvideIdempotencyToken>,
}
impl Config {
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
}
#[derive(Default)]
pub struct ConfigBuilder {
    #[allow(dead_code)]
    token_provider: Option<Box<dyn crate::idempotency_token::ProvideIdempotencyToken>>,
}
impl ConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn token_provider(
        mut self,
        token_provider: impl crate::idempotency_token::ProvideIdempotencyToken + 'static,
    ) -> Self {
        self.token_provider = Some(Box::new(token_provider));
        self
    }

    pub fn build(self) -> Config {
        Config {
            token_provider: self
            .token_provider
            .unwrap_or_else(|| Box::new(crate::idempotency_token::default_provider())),
        }
    }
}
 */
