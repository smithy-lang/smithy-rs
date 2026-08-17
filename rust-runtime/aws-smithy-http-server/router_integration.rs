#[cfg(test)]
mod differential {
    use super::rpc_v2_path::parse_rpc_v2_path;
    use regex::Regex;
    use std::sync::OnceLock;

    const IDENTIFIER_PATTERN: &str = r#"((_+([A-Za-z]|[0-9]))|[A-Za-z])[A-Za-z0-9_]*"#;

    fn legacy_regex() -> &'static Regex {
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| {
            Regex::new(&format!(
                r#"/service/({IDENTIFIER_PATTERN}\.)*(?P<service>{IDENTIFIER_PATTERN})/operation/(?P<operation>{IDENTIFIER_PATTERN})$"#
            ))
            .unwrap()
        })
    }

    fn legacy_parse(path: &str) -> Option<(&str, &str)> {
        let c = legacy_regex().captures(path)?;
        let (s, o) = (c.name("service")?, c.name("operation")?);
        Some((&path[s.range()], &path[o.range()]))
    }

    /// Deterministic splitmix-style generator; no `rand` dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    #[test]
    fn parser_matches_legacy_regex() {
        const ALPHABET: &[u8] = b"aZ09_./-#soperatinvc";
        const FRAGMENTS: &[&str] = &[
            "/service/",
            "/operation/",
            "service/",
            "operation",
            "com.example.",
            "Foo",
            "_",
            ".",
            "/",
            "Bar_1",
            "-",
            "#",
        ];
        let mut rng = Lcg(0x5EED);
        for it in 0..100_000 {
            let mut s = String::new();
            if it % 2 == 0 {
                let len = (rng.next() % 60) as usize;
                for _ in 0..len {
                    s.push(ALPHABET[(rng.next() as usize) % ALPHABET.len()] as char);
                }
            } else {
                for _ in 0..(rng.next() % 8) as usize {
                    s.push_str(FRAGMENTS[(rng.next() as usize) % FRAGMENTS.len()]);
                }
            }
            let expected = legacy_parse(&s);
            let got = parse_rpc_v2_path(&s).map(|p| (p.service, p.operation));
            assert_eq!(expected, got, "parser/regex mismatch on input {s:?}");
            // The zero-copy key invariant: route_key is the path tail from the
            // service name onward, i.e. "{service}/operation/{operation}".
            if let Some(p) = parse_rpc_v2_path(&s) {
                assert_eq!(
                    p.route_key,
                    format!("{}/operation/{}", p.service, p.operation)
                );
                assert!(s.ends_with(p.route_key));
            }
        }
    }
}
