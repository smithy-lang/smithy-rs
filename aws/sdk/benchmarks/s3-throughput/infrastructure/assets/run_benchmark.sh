#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

echo "run_benchmark.sh starting"
set -eux

BENCHMARK_BUCKET="${1}"
echo "benchmark bucket: ${BENCHMARK_BUCKET}"

COMMON_ARGS="--concurrency 30 --bucket ${BENCHMARK_BUCKET} --region us-east-1"

source "${HOME}/.cargo/env"
cd benchmark

# 1B
for fs in "tmpfs" "disk"; do
    BENCH_RESULT_FILE="bench_results_put_object_${fs}_1B.txt"
    cargo run --release -- --bench put-object --fs "${fs}" --size-bytes 1 ${COMMON_ARGS} &> "${BENCH_RESULT_FILE}"
    aws s3 cp "${BENCH_RESULT_FILE}" "s3://${BENCHMARK_BUCKET}/"

    BENCH_RESULT_FILE="bench_results_get_object_${fs}_1B.txt"
    cargo run --release -- --bench get-object --fs "${fs}" --size-bytes 1 ${COMMON_ARGS} &> "${BENCH_RESULT_FILE}"
    aws s3 cp "${BENCH_RESULT_FILE}" "s3://${BENCHMARK_BUCKET}/"
done

# multipart
for fs in "tmpfs" "disk"; do
    for size_bytes in "8388607" "8388609" "134217728" "4294967296" "32212254720"; do
        BENCH_RESULT_FILE="bench_results_put_object_multipart_${fs}_${size_bytes}B.txt"
        cargo run --release -- --bench put-object-multipart --fs "${fs}" --size-bytes "${size_bytes}" --part-size-bytes "8388608" ${COMMON_ARGS} &> "${BENCH_RESULT_FILE}"
        aws s3 cp "${BENCH_RESULT_FILE}" "s3://${BENCHMARK_BUCKET}/"

        BENCH_RESULT_FILE="bench_results_get_object_multipart_${fs}_${size_bytes}B.txt"
        cargo run --release -- --bench get-object-multipart --fs "${fs}" --size-bytes "${size_bytes}" --part-size-bytes "8388608" ${COMMON_ARGS} &> "${BENCH_RESULT_FILE}"
        aws s3 cp "${BENCH_RESULT_FILE}" "s3://${BENCHMARK_BUCKET}/"
    done
done

echo "Benchmark finished. Results have been uploaded to ${BENCHMARK_BUCKET}"
