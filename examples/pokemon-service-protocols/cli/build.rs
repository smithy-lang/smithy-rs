fn main() {
    let protocol_features = [
        ("rest-json1", "CARGO_FEATURE_REST_JSON1"),
        ("rest-xml", "CARGO_FEATURE_REST_XML"),
        ("aws-json-10", "CARGO_FEATURE_AWS_JSON_10"),
        ("aws-json-11", "CARGO_FEATURE_AWS_JSON_11"),
        ("rpcv2-cbor", "CARGO_FEATURE_RPCV2_CBOR"),
    ];

    let selected = protocol_features
        .iter()
        .filter_map(|(feature, env)| std::env::var_os(env).map(|_| *feature))
        .collect::<Vec<_>>();

    match selected.as_slice() {
        [_] => {}
        [] => panic!("select exactly one protocol feature"),
        many => panic!("select exactly one protocol feature; selected: {many:?}"),
    }
}
