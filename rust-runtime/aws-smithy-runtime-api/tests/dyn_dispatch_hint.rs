/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(feature = "client")]

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeSerializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
    FinalizerInterceptorContextRef,
};
use aws_smithy_runtime_api::client::interceptors::{dyn_dispatch_hint, Intercept, OverriddenHooks};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;

#[derive(Debug)]
struct SingleHook;

#[dyn_dispatch_hint]
impl Intercept for SingleHook {
    fn name(&self) -> &'static str {
        "SingleHook"
    }
    fn modify_before_signing(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        Ok(())
    }
}

#[test]
fn single_hook_sets_one_flag() {
    assert_eq!(
        SingleHook.overridden_hooks(),
        OverriddenHooks::MODIFY_BEFORE_SIGNING,
    );
}

#[derive(Debug)]
struct MultipleHooks;

#[dyn_dispatch_hint]
impl Intercept for MultipleHooks {
    fn name(&self) -> &'static str {
        "MultipleHooks"
    }
    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        Ok(())
    }
    fn read_after_execution(
        &self,
        _context: &FinalizerInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        Ok(())
    }
}

#[test]
fn multiple_hooks_or_flags_together() {
    let flags = MultipleHooks.overridden_hooks();
    let expected = OverriddenHooks::READ_BEFORE_EXECUTION | OverriddenHooks::READ_AFTER_EXECUTION;
    assert_eq!(
        flags, expected,
        "should contain exactly the two overridden hooks and nothing else"
    );
}

#[derive(Debug)]
struct NoHooks;

#[dyn_dispatch_hint]
impl Intercept for NoHooks {
    fn name(&self) -> &'static str {
        "NoHooks"
    }
}

#[test]
fn no_hooks_returns_none() {
    assert_eq!(NoHooks.overridden_hooks(), OverriddenHooks::none());
}

#[derive(Debug)]
struct NoMacro;

impl Intercept for NoMacro {
    fn name(&self) -> &'static str {
        "NoMacro"
    }
}

#[test]
fn without_macro_returns_all() {
    assert_eq!(NoMacro.overridden_hooks(), OverriddenHooks::all());
}

macro_rules! generate_hook {
    ($name:ident) => {
        fn $name(
            &self,
            _context: &mut BeforeTransmitInterceptorContextMut<'_>,
            _runtime_components: &RuntimeComponents,
            _cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            Ok(())
        }
    };
}

#[derive(Debug)]
struct MacroGeneratedHook;

#[dyn_dispatch_hint]
impl Intercept for MacroGeneratedHook {
    fn name(&self) -> &'static str {
        "MacroGeneratedHook"
    }
    generate_hook!(modify_before_signing);
}

/// A hook defined by a declarative macro is *not* detected, so it is never dispatched.
///
/// `#[dyn_dispatch_hint]` parses the impl block before declarative macros expand, so the body above
/// appears as an `ImplItem::Macro` rather than an `ImplItem::Fn`. The hook is implemented but gets
/// no bit in the mask, and `Interceptors` then skips it -- silently, with no warning.
///
/// This test pins that limitation rather than endorsing it. If the macro is ever taught to expand
/// declarative macros first, this assertion will fail; the fix is to expect
/// `OverriddenHooks::MODIFY_BEFORE_SIGNING` here and to delete the warning in the attribute's docs.
#[test]
fn macro_generated_hook_is_not_detected() {
    assert_eq!(
        MacroGeneratedHook.overridden_hooks(),
        OverriddenHooks::none(),
        "a macro-generated hook is invisible to the attribute; write hooks longhand"
    );
}
