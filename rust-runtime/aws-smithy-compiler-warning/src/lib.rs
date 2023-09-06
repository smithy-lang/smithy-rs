extern crate proc_macro;

#[proc_macro]
pub fn compiletime_sdk_error_warning(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    if cfg!(all(not(aws_sdk_unstable), any(feature = "serde-serialize", feature = "serde-deserialize"))) {
        let s = r#"
            You must pass `aws_sdk_unstable` flag to the compiler!
            e.g.
            ```
                export RUSTFLAGS="--cfg aws_sdk_unstable";
            ```
        "#;
        eprintln!("{s}");
    };
    ts
}
