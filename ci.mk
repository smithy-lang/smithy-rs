#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# This is a makefile executed by the `./ci` script that
# has a target for every single CI script in `tools/ci-scripts`,
# with dependencies between targets included so that it's not necessary
# to remember to generate a SDK for the targets that require one.

ARGS=
CI_BUILD=./smithy-rs/tools/ci-build
CI_ACTION=$(CI_BUILD)/ci-action

.PHONY: acquire-build-image
acquire-build-image:
	./smithy-rs/.github/scripts/acquire-build-image

.PHONY: check-aws-config
check-aws-config: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-canary
check-aws-sdk-canary: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-adhoc-tests
check-aws-sdk-adhoc-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-services
check-aws-sdk-services: generate-aws-sdk
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-only-aws-sdk-services
check-only-aws-sdk-services: generate-aws-sdk
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-smoketest-docs-clippy-udeps
check-aws-sdk-smoketest-docs-clippy-udeps: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-smoketest-unit-tests
check-aws-sdk-smoketest-unit-tests: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-standalone-integration-tests
check-aws-sdk-standalone-integration-tests: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-book
check-book: check-rust-runtimes
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-client-codegen-integration-tests
check-client-codegen-integration-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-client-codegen-unit-tests
check-client-codegen-unit-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-core-codegen-unit-tests
check-core-codegen-unit-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-rust-runtimes
check-rust-runtimes:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-tools
check-tools:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-sdk-codegen-unit-tests
check-sdk-codegen-unit-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-codegen-integration-tests
check-server-codegen-integration-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-codegen-unit-tests
check-server-codegen-unit-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-serde-codegen-unit-tests
check-serde-codegen-unit-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-codegen-integration-tests-python
check-server-codegen-integration-tests-python:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-codegen-unit-tests-python
check-server-codegen-unit-tests-python:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-e2e-test
check-server-e2e-test:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-python-e2e-test
check-server-python-e2e-test:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-style-and-lints
check-style-and-lints:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: generate-aws-sdk-smoketest
generate-aws-sdk-smoketest:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: generate-aws-sdk
generate-aws-sdk:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: generate-codegen-diff
generate-codegen-diff:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-deterministic-codegen
check-deterministic-codegen:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-semver
check-semver:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: generate-smithy-rs-release
generate-smithy-rs-release:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: verify-tls-config
verify-tls-config:
	$(CI_ACTION) $@ $(ARGS)
