ROOT := $(abspath ../../..)
GRADLE := $(ROOT)/gradlew
MODULES := request-limits-server-sdk,rpcv2Cbor_extras,rpcv2Cbor_extras_no_initial_response,rpcv2Cbor_extras-http0x,rpcv2Cbor_extras_no_initial_response-http0x

.PHONY: all codegen build run test
all: build

codegen:
	$(GRADLE) --project-dir $(ROOT) -Pmodules=$(MODULES) :codegen-server-test:assemble

build: codegen
	cargo build -p request-limits --bin request-limits-server

run: codegen
	cargo run -p request-limits --bin request-limits-server -- $(ARGS)

test: codegen
	cargo test -p request-limits
