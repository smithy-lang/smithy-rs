/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Criterion benchmarks for resolving XML structure fields to members.
//!
//! Structure sizes follow member counts across AWS models (p50 = 2, p90 = 6,
//! p99 = 20, max about 42). Fields are emitted in model order, as serializers do,
//! except for the `sparse` case (every third member, as when optional members are
//! omitted) and the `reversed` case (worst case for ordered matching).

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use aws_smithy_xml::codec::XmlCodec;

/// Member names taken from SSM, 42 being the widest structure in AWS models.
const NAMES: [&str; 42] = [
    "Name",
    "Value",
    "Type",
    "KeyId",
    "Overwrite",
    "Description",
    "AllowedPattern",
    "Tags",
    "Tier",
    "Policies",
    "DataType",
    "DocumentName",
    "DocumentVersion",
    "DocumentHash",
    "DocumentHashType",
    "TimeoutSeconds",
    "Comment",
    "InstanceIds",
    "Targets",
    "Parameters",
    "MaxConcurrency",
    "MaxErrors",
    "ServiceRoleArn",
    "NotificationConfig",
    "CloudWatchOutputConfig",
    "AlarmConfiguration",
    "OutputS3Region",
    "OutputS3BucketName",
    "OutputS3KeyPrefix",
    "CloudWatchOutputEnabled",
    "CloudWatchLogGroupName",
    "NotificationArn",
    "NotificationEvents",
    "NotificationType",
    "IgnorePollAlarmFailure",
    "Alarms",
    "TargetCount",
    "CompletedCount",
    "ErrorCount",
    "DeliveryTimedOutCount",
    "CommandId",
    "StatusDetails",
];

macro_rules! members {
    ($($static_name:ident = $index:literal),* $(,)?) => {
        $(
            static $static_name: Schema<'static> = Schema::new_member(
                shape_id!("bench", "Wide", "m"),
                ShapeType::String,
                NAMES[$index],
                $index,
            );
        )*
    };
}

members!(
    M00 = 0,
    M01 = 1,
    M02 = 2,
    M03 = 3,
    M04 = 4,
    M05 = 5,
    M06 = 6,
    M07 = 7,
    M08 = 8,
    M09 = 9,
    M10 = 10,
    M11 = 11,
    M12 = 12,
    M13 = 13,
    M14 = 14,
    M15 = 15,
    M16 = 16,
    M17 = 17,
    M18 = 18,
    M19 = 19,
    M20 = 20,
    M21 = 21,
    M22 = 22,
    M23 = 23,
    M24 = 24,
    M25 = 25,
    M26 = 26,
    M27 = 27,
    M28 = 28,
    M29 = 29,
    M30 = 30,
    M31 = 31,
    M32 = 32,
    M33 = 33,
    M34 = 34,
    M35 = 35,
    M36 = 36,
    M37 = 37,
    M38 = 38,
    M39 = 39,
    M40 = 40,
    M41 = 41,
);

static S2: Schema<'static> = Schema::new_struct(
    shape_id!("bench", "S2"),
    ShapeType::Structure,
    &[&M00, &M01],
);
static S6: Schema<'static> = Schema::new_struct(
    shape_id!("bench", "S6"),
    ShapeType::Structure,
    &[&M00, &M01, &M02, &M03, &M04, &M05],
);
static S20: Schema<'static> = Schema::new_struct(
    shape_id!("bench", "S20"),
    ShapeType::Structure,
    &[
        &M00, &M01, &M02, &M03, &M04, &M05, &M06, &M07, &M08, &M09, &M10, &M11, &M12, &M13, &M14,
        &M15, &M16, &M17, &M18, &M19,
    ],
);
static S42: Schema<'static> = Schema::new_struct(
    shape_id!("bench", "S42"),
    ShapeType::Structure,
    &[
        &M00, &M01, &M02, &M03, &M04, &M05, &M06, &M07, &M08, &M09, &M10, &M11, &M12, &M13, &M14,
        &M15, &M16, &M17, &M18, &M19, &M20, &M21, &M22, &M23, &M24, &M25, &M26, &M27, &M28, &M29,
        &M30, &M31, &M32, &M33, &M34, &M35, &M36, &M37, &M38, &M39, &M40, &M41,
    ],
);

/// Every benchmarked case: name, structure schema and the field order on the wire.
pub fn cases() -> Vec<(String, &'static Schema<'static>, Vec<u8>)> {
    let mut cases = Vec::new();
    for (size, schema) in [(2, &S2), (6, &S6), (20, &S20), (42, &S42)] {
        let fields: Vec<&str> = NAMES[..size].to_vec();
        cases.push((format!("struct_{size}"), schema, encode(schema, &fields)));
    }
    let sparse: Vec<&str> = NAMES.iter().copied().step_by(3).collect();
    cases.push(("struct_42/sparse".into(), &S42, encode(&S42, &sparse)));
    let reversed: Vec<&str> = NAMES[..20].iter().rev().copied().collect();
    cases.push(("struct_20/reversed".into(), &S20, encode(&S20, &reversed)));
    cases
}

fn encode(schema: &Schema<'_>, fields: &[&str]) -> Vec<u8> {
    let root = schema.shape_id().shape_name();
    let body: String = fields
        .iter()
        .map(|field| format!("<{field}>value-0123456789</{field}>"))
        .collect();
    format!("<{root}>{body}</{root}>").into_bytes()
}

/// Reads every field of `schema` as a string and returns the number of fields read.
#[inline(never)]
pub fn deserialize(schema: &'static Schema<'static>, input: &[u8]) -> usize {
    let codec = XmlCodec::default();
    let mut deserializer = codec.create_deserializer(input);
    let mut fields = 0;
    deserializer
        .read_struct(schema, &mut |member, d| {
            black_box(d.read_string(member)?);
            fields += 1;
            Ok(())
        })
        .unwrap();
    fields
}

fn bench(c: &mut Criterion) {
    for (name, schema, input) in cases() {
        c.bench_function(&format!("deserialize/{name}"), |b| {
            b.iter(|| deserialize(black_box(schema), black_box(&input)))
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
