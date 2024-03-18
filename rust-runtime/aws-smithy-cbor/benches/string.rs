use aws_smithy_cbor::decode::{Decoder, DeserializeError};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

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

    c.bench_function("definite string using vec::concat", |b| {
        b.iter(|| {
            let mut decoder = minicbor::Decoder::new(&definite_bytes);
            let iter = decoder.str_iter().expect("miniCBor could not decode bytes");
            let parts: Vec<&str> = iter.collect::<Result<_, _>>().unwrap();
            let _: String = parts.concat();
        })
    });

    c.bench_function("indefinite string using vec::concat", |b| {
        b.iter(|| {
            let mut decoder = minicbor::Decoder::new(&indefinite_bytes);
            let iter = decoder.str_iter().expect("miniCBor could not decode bytes");
            let parts: Vec<&str> = iter.collect::<Result<_, _>>().unwrap();
            let _: String = parts.concat();
        })
    });

    c.bench_function("definite string using next()", |b| {
        b.iter(|| {
            let mut decoder = minicbor::Decoder::new(&definite_bytes);
            let mut iter = decoder.str_iter().expect("miniCBor could not decode bytes");
            let head = iter.next().unwrap().unwrap();
            let rest = iter.next();

            if let Some(rest) = rest {
                let second = rest.unwrap();
                let mut collection = vec![head, second];
                for r in iter {
                    collection.push(r.unwrap());
                }
                let key = collection.concat();
                let _key = black_box(std::borrow::Cow::from(key));
            } else {
                let _key = black_box(std::borrow::Cow::from(head));
            }
        })
    });

    c.bench_function("indefinite string using next()", |b| {
        b.iter(|| {
            let mut decoder = minicbor::Decoder::new(&indefinite_bytes);
            let mut iter = decoder.str_iter().expect("miniCBor could not decode bytes");
            let head = iter.next().unwrap().unwrap();
            let rest = iter.next();

            if let Some(rest) = rest {
                let second = rest.unwrap();
                let mut collection = vec![head, second];
                for r in iter {
                    collection.push(r.unwrap());
                }
                let key = collection.concat();
                let _key = black_box(std::borrow::Cow::from(key));
            } else {
                let _key = black_box(std::borrow::Cow::from(head));
            }
        })
    });

    c.bench_function("definite string using next() with a String", |b| {
        b.iter(|| {
            let mut decoder = minicbor::Decoder::new(&definite_bytes);
            let mut iter = decoder.str_iter().expect("miniCBor could not decode bytes");
            let head = iter.next().unwrap().unwrap();
            let rest = iter.next();

            if let Some(rest) = rest {
                let mut head = String::from(head);
                head.push_str(rest.unwrap());
                for r in iter {
                    head.push_str(r.unwrap());
                }
                let _key = black_box(std::borrow::Cow::from(head));
            } else {
                let _key = black_box(std::borrow::Cow::from(head));
            }
        })
    });

    c.bench_function("indefinite string using next() with a String", |b| {
        b.iter(|| {
            let mut decoder = minicbor::Decoder::new(&indefinite_bytes);
            let mut iter = decoder.str_iter().expect("miniCBor could not decode bytes");
            let head = iter.next().unwrap().unwrap();
            let rest = iter.next();

            if let Some(rest) = rest {
                let mut head = String::from(head);
                head.push_str(rest.unwrap());
                for r in iter {
                    head.push_str(r.unwrap());
                }
                let _key = black_box(std::borrow::Cow::from(head));
            } else {
                let _key = black_box(std::borrow::Cow::from(head));
            }
        })
    });

    // c.bench_function("definite string using into", |b| {
    //     b.iter(|| {
    //         let mut decoder = minicbor::Decoder::new(&bytes);
    //         let iter = decoder.str_iter().expect("miniCBor could not decode bytes");
    //         let parts: Vec<&str> = iter.collect::<Result<_, _>>().unwrap();
    //         let _: String = if parts.len() == 1 {
    //             parts[0].into() // Directly convert &str to String if there's only one part
    //         } else {
    //             parts.concat() // Concatenate all parts into a single String
    //         };
    //     })
    // });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
