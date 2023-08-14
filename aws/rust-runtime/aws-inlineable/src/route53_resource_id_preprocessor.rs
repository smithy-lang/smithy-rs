/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(dead_code)]

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeSerializationInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Interceptor;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use std::fmt;
use std::marker::PhantomData;

// This function is only used to strip prefixes from resource IDs at the time they're passed as
// input to a request. Resource IDs returned in responses may or may not include a prefix.
/// Strip the resource type prefix from resource ID return
fn trim_resource_id(resource_id: &mut String) {
    const PREFIXES: &[&str] = &[
        "/hostedzone/",
        "hostedzone/",
        "/change/",
        "change/",
        "/delegationset/",
        "delegationset/",
    ];

    for prefix in PREFIXES {
        if let Some(trimmed_id) = resource_id.strip_prefix(prefix) {
            *resource_id = trimmed_id.to_string();
            return;
        }
    }
}

pub(crate) struct Route53ResourceIdInterceptor<G, T>
where
    G: for<'a> Fn(&'a mut T) -> Option<&'a mut String>,
{
    get_mut_resource_id: G,
    _phantom: PhantomData<T>,
}

impl<G, T> fmt::Debug for Route53ResourceIdInterceptor<G, T>
where
    G: for<'a> Fn(&'a mut T) -> Option<&'a mut String>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Route53ResourceIdInterceptor").finish()
    }
}

impl<G, T> Route53ResourceIdInterceptor<G, T>
where
    G: for<'a> Fn(&'a mut T) -> Option<&'a mut String>,
{
    pub(crate) fn new(get_mut_resource_id: G) -> Self {
        Self {
            get_mut_resource_id,
            _phantom: Default::default(),
        }
    }
}

impl<G, T> Interceptor for Route53ResourceIdInterceptor<G, T>
where
    G: for<'a> Fn(&'a mut T) -> Option<&'a mut String> + Send + Sync,
    T: fmt::Debug + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        "Route53ResourceIdInterceptor"
    }

    fn modify_before_serialization(
        &self,
        context: &mut BeforeSerializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let input: &mut T = context.input_mut().downcast_mut().expect("correct type");
        if let Some(field) = (self.get_mut_resource_id)(input) {
            trim_resource_id(field)
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::trim_resource_id;

    #[test]
    fn does_not_change_regular_zones() {
        struct OperationInput {
            resource: String,
        }

        let mut operation = OperationInput {
            resource: "Z0441723226OZ66S5ZCNZ".to_string(),
        };
        trim_resource_id(&mut operation.resource);
        assert_eq!(&operation.resource, "Z0441723226OZ66S5ZCNZ");
    }

    #[test]
    fn sanitizes_prefixed_zone() {
        struct OperationInput {
            change_id: String,
        }

        let mut operation = OperationInput {
            change_id: "/change/Z0441723226OZ66S5ZCNZ".to_string(),
        };
        trim_resource_id(&mut operation.change_id);
        assert_eq!(&operation.change_id, "Z0441723226OZ66S5ZCNZ");
    }

    #[test]
    fn allow_no_leading_slash() {
        struct OperationInput {
            hosted_zone: String,
        }

        let mut operation = OperationInput {
            hosted_zone: "hostedzone/Z0441723226OZ66S5ZCNZ".to_string(),
        };
        trim_resource_id(&mut operation.hosted_zone);
        assert_eq!(&operation.hosted_zone, "Z0441723226OZ66S5ZCNZ");
    }
}
