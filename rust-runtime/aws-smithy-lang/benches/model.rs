/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Parse, write, traversal, knowledge-index, and runtime schema benchmarks for SQS, S3, and
//! EC2.
//! See `benches/README.md` for methodology and the recorded baseline.

use aws_smithy_lang::traversal::Walker;
use aws_smithy_lang::{traits, AstWriter, Model, ShapeId};
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::path::PathBuf;

const MODELS: [&str; 3] = ["sqs", "s3", "ec2"];

fn read(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../aws/sdk/aws-models")
        .join(format!("{name}.json"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn bench(c: &mut Criterion) {
    let inputs: Vec<_> = MODELS.iter().map(|name| (*name, read(name))).collect();

    let mut group = c.benchmark_group("parse");
    for (name, bytes) in &inputs {
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), bytes, |b, bytes| {
            b.iter(|| Model::from_json_slice(name, black_box(bytes)).unwrap())
        });
    }
    group.finish();

    let models: Vec<_> = inputs
        .iter()
        .map(|(name, bytes)| (*name, Model::from_json_slice(name, bytes).unwrap()))
        .collect();

    let mut group = c.benchmark_group("write");
    let writer = AstWriter::new();
    for (name, model) in &models {
        group.bench_with_input(BenchmarkId::from_parameter(name), model, |b, model| {
            let mut out = Vec::with_capacity(8 << 20);
            b.iter(|| {
                out.clear();
                writer.write(black_box(model), &mut out).unwrap();
                out.len()
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("service_closure");
    for (name, model) in &models {
        let service = model.services().next().expect("one service per model");
        group.bench_with_input(BenchmarkId::from_parameter(name), &service, |b, service| {
            b.iter(|| Walker::new().walk(black_box(*service)).count())
        });
    }
    group.finish();

    // Building the relationship index needs a model whose cache is still empty.
    let mut group = c.benchmark_group("relationship_index_build");
    for (name, bytes) in &inputs {
        group.bench_with_input(BenchmarkId::from_parameter(name), bytes, |b, bytes| {
            b.iter_batched(
                || Model::from_json_slice(name, bytes).unwrap(),
                |model| {
                    black_box(model.knowledge().relationships());
                    model
                },
                BatchSize::LargeInput,
            )
        });
    }
    group.finish();

    // The remaining queries run against an initialized cache.
    for (_, model) in &models {
        model.knowledge().relationships();
    }

    let mut group = c.benchmark_group("contained_operations");
    for (name, model) in &models {
        let service = model.services().next().unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &service, |b, service| {
            b.iter(|| black_box(*service).contained_operations().len())
        });
    }
    group.finish();

    let mut group = c.benchmark_group("incoming_relationships");
    for (name, model) in &models {
        let relationships = model.knowledge().relationships();
        let string = ShapeId::from_static("smithy.api#String");
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| {
                relationships
                    .get_incoming(black_box(&string))
                    .unwrap()
                    .count()
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("input_bindings");
    for (name, model) in &models {
        let service = model.services().next().unwrap();
        let input = service.contained_operations().next().unwrap().input();
        let operations = model.knowledge().operations();
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| operations.input_bindings(black_box(input)).unwrap().len())
        });
    }
    group.finish();

    let mut group = c.benchmark_group("parent_chain");
    for (name, model) in &models {
        let service = model.services().next().unwrap();
        // The deepest operation, to measure the slowest chain lookup.
        let bottom_up = model.knowledge().bottom_up();
        let operation = service
            .contained_operations()
            .max_by_key(|op| {
                bottom_up
                    .parent_chain(service.id(), op.id())
                    .ok()
                    .flatten()
                    .map_or(0, |chain| chain.len())
            })
            .unwrap();
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| {
                bottom_up
                    .parent_chain(service.id(), black_box(operation.id()))
                    .unwrap()
                    .unwrap()
                    .len()
            })
        });
    }
    group.finish();
}

/// Shape ID construction and trait lookup
fn ids_and_traits(c: &mut Criterion) {
    let mut group = c.benchmark_group("shape_id");
    group.bench_function("parse", |b| {
        b.iter(|| ShapeId::new(black_box("com.amazonaws.s3#PutObjectRequest")).unwrap())
    });
    group.bench_function("from_parts", |b| {
        b.iter(|| {
            ShapeId::from_parts(
                black_box("com.amazonaws.s3"),
                black_box("PutObjectRequest"),
                None,
            )
            .unwrap()
        })
    });
    let root = ShapeId::new("com.amazonaws.s3#PutObjectRequest").unwrap();
    group.bench_function("with_member", |b| {
        b.iter(|| black_box(&root).with_member(black_box("Bucket")).unwrap())
    });
    group.bench_function("namespace", |b| {
        b.iter(|| black_box(&root).namespace().len())
    });
    group.finish();

    // Trait lookup on every root shape and member of S3, by a static and a parsed ShapeId.
    let model = Model::from_json_slice("s3", &read("s3")).unwrap();
    let mut group = c.benchmark_group("trait_lookup/s3");
    let count = |has: &dyn Fn(&aws_smithy_lang::ShapeRef<'_>) -> bool| {
        model
            .shapes()
            .flat_map(|s| {
                std::iter::once(aws_smithy_lang::ShapeRef::from(s))
                    .chain(s.members().iter().map(Into::into))
            })
            .filter(|s| has(s))
            .count()
    };
    group.bench_function("shape_id", |b| {
        b.iter(|| count(&|s| s.has_trait(black_box(&traits::DOCUMENTATION))))
    });
    let parsed = ShapeId::new("smithy.api#documentation").unwrap();
    group.bench_function("parsed_shape_id", |b| {
        b.iter(|| count(&|s| s.has_trait(black_box(&parsed))))
    });
    group.finish();

    // Every member of every S3 shape, looked up by name and by index.
    let mut group = c.benchmark_group("member_lookup/s3");
    let members: Vec<_> = model
        .shapes()
        .flat_map(|s| s.members().iter().map(move |m| (s, m.name(), m.index())))
        .collect();
    group.bench_function("name", |b| {
        b.iter(|| {
            members
                .iter()
                .filter(|(s, name, _)| s.member(black_box(name)).is_some())
                .count()
        })
    });
    group.bench_function("index", |b| {
        b.iter(|| {
            members
                .iter()
                .filter(|(s, _, index)| s.member_at(black_box(*index)).is_some())
                .count()
        })
    });
    group.finish();
}

/// Runtime schema construction and lookup. Requires `--features runtime-schema`.
#[cfg(feature = "runtime-schema")]
fn runtime_schemas(c: &mut Criterion) {
    use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;

    let models: Vec<_> = MODELS
        .iter()
        .map(|name| (*name, Model::from_json_slice(name, &read(name)).unwrap()))
        .collect();

    let mut group = c.benchmark_group("schema/all");
    for (name, model) in &models {
        group.bench_with_input(BenchmarkId::from_parameter(name), model, |b, model| {
            b.iter(|| RuntimeSchemaSet::try_from(black_box(model)).unwrap())
        });
    }
    group.finish();

    // Service selection walks the service's declarations rather than the knowledge index, so
    // warm and cold knowledge should now measure the same; both are kept for comparison with
    // earlier results.
    let mut group = c.benchmark_group("schema/service");
    for (name, model) in &models {
        let service = model.services().next().unwrap().id().clone();
        model.knowledge().relationships();
        group.bench_with_input(BenchmarkId::from_parameter(name), model, |b, model| {
            b.iter(|| {
                RuntimeSchemaSet::builder(black_box(model))
                    .for_service(&service)
                    .build()
                    .unwrap()
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("schema/service_cold_knowledge");
    for name in MODELS {
        let bytes = read(name);
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter_batched(
                || Model::from_json_slice(name, &bytes).unwrap(),
                |model| {
                    let service = model.services().next().unwrap().id().clone();
                    let set = RuntimeSchemaSet::builder(&model)
                        .for_service(&service)
                        .build()
                        .unwrap();
                    (set, model)
                },
                BatchSize::LargeInput,
            )
        });
    }
    group.finish();

    // A filtered service scope with the first 1 and 10 operations in service order.
    let mut group = c.benchmark_group("schema/filtered");
    for (name, model) in &models {
        let service = model.services().next().unwrap();
        let ids: Vec<_> = service
            .contained_operations()
            .map(|operation| operation.id().clone())
            .collect();
        for count in [1, 10] {
            let selected = &ids[..count.min(ids.len())];
            group.bench_function(BenchmarkId::new(*name, count), |b| {
                b.iter(|| {
                    RuntimeSchemaSet::builder(black_box(model))
                        .for_service_operations(service.id(), selected)
                        .build()
                        .unwrap()
                })
            });
        }
    }
    group.finish();

    let mut group = c.benchmark_group("schema/operation_lookup");
    for (name, model) in &models {
        let set = RuntimeSchemaSet::try_from(model).unwrap();
        let operation = model.operations().next().unwrap().id().clone();
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| {
                set.operation(black_box(&operation))
                    .unwrap()
                    .input()
                    .members()
                    .len()
            })
        });
    }
    group.finish();

    // The complete descriptor, touching every field so none is optimized away.
    let mut group = c.benchmark_group("schema/operation_descriptor");
    for (name, model) in &models {
        let set = RuntimeSchemaSet::try_from(model).unwrap();
        let operation = model.operations().next().unwrap().id().clone();
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| {
                let descriptor = black_box(set.operation(black_box(&operation)).unwrap());
                (
                    descriptor.id().as_str().len(),
                    descriptor.input().members().len(),
                    descriptor.output().members().len(),
                    descriptor.errors().len(),
                )
            })
        });
    }
    group.finish();
}

#[cfg(not(feature = "runtime-schema"))]
fn runtime_schemas(_: &mut Criterion) {}

criterion_group!(benches, bench, ids_and_traits, runtime_schemas);
criterion_main!(benches);
