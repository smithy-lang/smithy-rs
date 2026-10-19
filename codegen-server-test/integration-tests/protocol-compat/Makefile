SRC_DIR := $(shell git rev-parse --show-toplevel)
GRADLE := $(SRC_DIR)/gradlew
COMPAT := compat-awsjson11-server-sdk,compat-awsjson11-legacy-server-sdk,compat-xml-server-sdk,compat-xml-legacy-server-sdk,compat-rest-server-sdk,compat-rest-cbor-server-sdk,compat-rest-awsjson-server-sdk,compat-rest-xml-server-sdk,compat-awsjson-server-sdk,compat-awsjson-spec-server-sdk,compat-cbor-server-sdk,compat-all-server-sdk,compat-document-server-sdk,compat-rest-legacy-server-sdk,compat-awsjson-legacy-server-sdk,compat-cbor-legacy-server-sdk,compat-document-legacy-server-sdk
# Cargo loads every workspace member's manifest, including the sibling suites.
MODULES := $(COMPAT),multi-protocol-server-sdk,multi-protocol-client,multi-protocol-client-rpcv2cbor,multi-protocol-client-awsjson10,multi-protocol-client-awsjson11,multi-protocol-client-restjson,multi-protocol-client-restxml,rpcv2Cbor_extras,rpcv2Cbor_extras_no_initial_response,rpcv2Cbor_extras-http0x,rpcv2Cbor_extras_no_initial_response-http0x

all: test

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='$(MODULES)' \
		:codegen-client-test:generateSmithyBuild --rerun :codegen-client-test:smithyBuild --rerun \
		:codegen-server-test:generateSmithyBuild --rerun :codegen-server-test:smithyBuild --rerun \
		:codegen-client-test:assemble :codegen-server-test:assemble

test: codegen
	cargo test --quiet -p protocol-compat

test-runtime-client: codegen
	cargo test --quiet -p protocol-compat --test runtime_protocol_clients

.PHONY: all codegen test test-runtime-client
