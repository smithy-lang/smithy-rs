SRC_DIR := $(shell git rev-parse --show-toplevel)
GRADLE := $(SRC_DIR)/gradlew
MODULES := event-stream-bench-rest-json-legacy,event-stream-bench-rest-json-schema,event-stream-bench-rpc-v2-cbor-legacy,event-stream-bench-rpc-v2-cbor-schema,event-stream-bench-multi-schema,event-stream-bench-multi-rest-json-schema
PRESET ?= small
EVENT_COUNT ?=
TRANSPORT ?= tower
MODE ?= server

all: test

codegen:
	$(GRADLE) --project-dir $(SRC_DIR) -P modules='$(MODULES)' \
		:codegen-server-test:generateSmithyBuild --rerun \
		:codegen-server-test:generateCargoWorkspace --rerun \
		:codegen-server-test:smithyBuild --rerun \
		:codegen-server-test:assemble
	python3 expose_serde.py

test: codegen
	cargo test --quiet
	BENCH_MODE=server BENCH_PRESET=full cargo bench --quiet --bench event_streams -- --test
	BENCH_MODE=serde BENCH_PRESET=full cargo bench --quiet --bench event_streams -- --test

bench: codegen
	BENCH_MODE="$(MODE)" BENCH_TRANSPORT="$(TRANSPORT)" BENCH_PRESET="$(PRESET)" BENCH_EVENT_COUNT="$(EVENT_COUNT)" cargo bench --quiet --bench event_streams -- $(BENCH_ARGS)

.PHONY: all codegen test bench
