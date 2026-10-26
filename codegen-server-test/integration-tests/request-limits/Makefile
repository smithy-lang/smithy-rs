ROOT := $(abspath ../../..)
GRADLE := $(ROOT)/gradlew
MODULES := request-limits-server-sdk,rpcv2Cbor_extras,rpcv2Cbor_extras_no_initial_response,rpcv2Cbor_extras-http0x,rpcv2Cbor_extras_no_initial_response-http0x

.PHONY: all codegen test
all: test

codegen:
	$(GRADLE) --project-dir $(ROOT) -Pmodules=$(MODULES) :codegen-server-test:assemble

test: codegen
	cargo test --quiet -p request-limits
