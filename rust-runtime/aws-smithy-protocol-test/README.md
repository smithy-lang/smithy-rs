# Smithy Protocol Tests

This library implements utilities for validating serializers & deserializers
against [Smithy protocol tests](https://awslabs.github.io/smithy/1.0/spec/http-protocol-compliance-tests.html). Specifically, this crate includes support for:

* MediaType-aware comparison for XML, JSON and AWS Query.
* NaN/Infinty supporting floating point comparisons.
* HTTP header & query string validators.
