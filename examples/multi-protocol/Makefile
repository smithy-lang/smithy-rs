SRC_DIR := $(shell git rev-parse --show-toplevel)
GRADLE := $(SRC_DIR)/gradlew
MODULES := multi-protocol-server-sdk,multi-protocol-client-rpcv2cbor,multi-protocol-client-awsjson10,multi-protocol-client-awsjson11,multi-protocol-client-restjson,multi-protocol-client-restxml

all: test

# Generates the server serving every protocol and one client per protocol. `smithyBuild` does not track
# imported models as inputs, so it is rerun to pick up model edits.
codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='$(MODULES)' \
		:codegen-client-test:smithyBuild --rerun :codegen-server-test:smithyBuild --rerun \
		:codegen-client-test:assemble :codegen-server-test:assemble

test: codegen
	cargo test

.PHONY: all codegen test
