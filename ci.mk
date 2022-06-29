#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# This is a makefile executed by the `./ci` script that
# has a target for every single CI script in `tools/ci-build/scripts`,
# with dependencies between targets included so that it's not necessary
# to remember to generate a SDK for the targets that require one.

ARGS=
CI_BUILD=./smithy-rs/tools/ci-build
CI_ACTION=$(CI_BUILD)/ci-action

.PHONY: acquire-build-image
acquire-build-image:
	$(CI_BUILD)/acquire-build-image

.PHONY: check-aws-sdk-examples
check-aws-sdk-examples: generate-aws-sdk
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-services
check-aws-sdk-services: generate-aws-sdk
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-smoketest-additional-checks
check-aws-sdk-smoketest-additional-checks: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-smoketest-docs-clippy-udeps
check-aws-sdk-smoketest-docs-clippy-udeps: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-aws-sdk-smoketest-unit-tests
check-aws-sdk-smoketest-unit-tests: generate-aws-sdk-smoketest
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-client-codegen-integration-tests
check-client-codegen-integration-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-client-codegen-unit-tests
check-client-codegen-unit-tests:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-rust-runtimes-and-tools
check-rust-runtimes-and-tools: generate-aws-sdk-smoketest
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

.PHONY: check-server-codegen-integration-tests-python
check-server-codegen-integration-tests-python:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-codegen-unit-tests-python
check-server-codegen-unit-tests-python:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: check-server-e2e-test
check-server-e2e-test:
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

.PHONY: generate-smithy-rs-release
generate-smithy-rs-release:
	$(CI_ACTION) $@ $(ARGS)

.PHONY: sanity-test
sanity-test:
	$(CI_ACTION) $@ $(ARGS)
