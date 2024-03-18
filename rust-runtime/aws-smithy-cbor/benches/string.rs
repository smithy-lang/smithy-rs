use aws_smithy_cbor::decode::{Decoder, DeserializeError};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// let mut decoder = minicbor::Decoder::new(&definite_bytes);
// let iter = decoder.str_iter().expect("miniCBor could not decode bytes");
// let parts: Vec<&str> = iter.collect::<Result<_, _>>().unwrap();
// let _: String = parts.concat();

pub fn criterion_benchmark(c: &mut Criterion) {
    // Definite length key `thisIsAKey`.
    let definite_bytes = [
        0x6a, 0x74, 0x68, 0x69, 0x73, 0x49, 0x73, 0x41, 0x4b, 0x65, 0x79,
    ];

    // Indefinite length key `this`, `Is`, `A` and `Key`.
    let indefinite_bytes = [
        0x7f, 0x64, 0x74, 0x68, 0x69, 0x73, 0x62, 0x49, 0x73, 0x61, 0x41, 0x63, 0x4b, 0x65, 0x79,
        0xff,
    ];

    c.bench_function("definite string", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&definite_bytes);
            let _ = black_box(decoder.str());
        })
    });
    c.bench_function("indefinite string", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&indefinite_bytes);
            let _ = black_box(decoder.str());
        })
    });

    c.bench_function("definite string alt", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&definite_bytes);
            let _ = black_box(decoder.str_alt());
        })
    });
    c.bench_function("indefinite string alt", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&indefinite_bytes);
            let _ = black_box(decoder.str_alt());
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
