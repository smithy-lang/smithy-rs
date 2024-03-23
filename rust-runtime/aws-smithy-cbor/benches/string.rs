use aws_smithy_cbor::decode::{Decoder, DeserializeError};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

pub fn str_benchmark(c: &mut Criterion) {
    // Definite length key `thisIsAKey`.
    let definite_bytes = [
        0x6a, 0x74, 0x68, 0x69, 0x73, 0x49, 0x73, 0x41, 0x4b, 0x65, 0x79,
    ];

    // Indefinite length key `this`, `Is`, `A` and `Key`.
    let indefinite_bytes = [
        0x7f, 0x64, 0x74, 0x68, 0x69, 0x73, 0x62, 0x49, 0x73, 0x61, 0x41, 0x63, 0x4b, 0x65, 0x79,
        0xff,
    ];

    c.bench_function("definite str_alt", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&definite_bytes);
            let x = black_box(decoder.str_alt());
            assert!(matches!(x.unwrap().as_ref(), "thisIsAKey"));
        })
    });
    c.bench_function("indefinite str_alt", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&indefinite_bytes);
            let x = black_box(decoder.str_alt());
            assert!(matches!(x.unwrap().as_ref(), "thisIsAKey"));
        })
    });

    c.bench_function("definite str()", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&definite_bytes);
            let x = black_box(decoder.str());
            assert!(matches!(x.unwrap().as_ref(), "thisIsAKey"));
        })
    });
    c.bench_function("indefinite str()", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&indefinite_bytes);
            let x = black_box(decoder.str());
            assert!(matches!(x.unwrap().as_ref(), "thisIsAKey"));
        })
    });
}

pub fn string_benchmark(c: &mut Criterion) {
    // Definite length key `thisIsAKey`.
    let definite_bytes = [
        0x6a, 0x74, 0x68, 0x69, 0x73, 0x49, 0x73, 0x41, 0x4b, 0x65, 0x79,
    ];

    // Indefinite length key `this`, `Is`, `A` and `Key`.
    let indefinite_bytes = [
        0x7f, 0x64, 0x74, 0x68, 0x69, 0x73, 0x62, 0x49, 0x73, 0x61, 0x41, 0x63, 0x4b, 0x65, 0x79,
        0xff,
    ];

    c.bench_function("definite string()", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&definite_bytes);
            let _ = black_box(decoder.str());
        })
    });

    c.bench_function("indefinite string()", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&indefinite_bytes);
            let _ = black_box(decoder.str());
        })
    });

    c.bench_function("definite string_alt()", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&definite_bytes);
            let _ = black_box(decoder.str_alt());
        })
    });

    c.bench_function("indefinite string_alt()", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&indefinite_bytes);
            let _ = black_box(decoder.str_alt());
        })
    });
}

pub fn blob_benchmark(c: &mut Criterion) {
    // Indefinite length blob containing bytes corresponding to `indefinite-byte, chunked, on each comma`.
    let blob_indefinite_bytes = [
        0x5f, 0x50, 0x69, 0x6e, 0x64, 0x65, 0x66, 0x69, 0x6e, 0x69, 0x74, 0x65, 0x2d, 0x62, 0x79,
        0x74, 0x65, 0x2c, 0x49, 0x20, 0x63, 0x68, 0x75, 0x6e, 0x6b, 0x65, 0x64, 0x2c, 0x4e, 0x20,
        0x6f, 0x6e, 0x20, 0x65, 0x61, 0x63, 0x68, 0x20, 0x63, 0x6f, 0x6d, 0x6d, 0x61, 0xff,
    ];

    c.bench_function("blob", |b| {
        b.iter(|| {
            let mut decoder = Decoder::new(&blob_indefinite_bytes);
            let _ = black_box(decoder.blob());
        })
    });

    // c.bench_function("blob-alt", |b| {
    //     b.iter(|| {
    //         let mut decoder = Decoder::new(&blob_indefinite_bytes);
    //         let _ = black_box(decoder.blob_alt());
    //     })
    // });
}

criterion_group!(benches, string_benchmark, /*str_benchmark blob_benchmark */,);

criterion_main!(benches);
