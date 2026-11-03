SRC_DIR := $(shell git rev-parse --show-toplevel)
GRADLE := $(SRC_DIR)/gradlew

all: test

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) generateServerCompatibilitySdk

test:
	$(GRADLE) --project-dir $(SRC_DIR) :codegen-server-test:protocolCompatibilityTest

test-runtime-client: codegen
	cargo test --quiet --test runtime_protocol_clients

.PHONY: all codegen test test-runtime-client
