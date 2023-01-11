object UnstableDerive {
    // double `#` is because this data is passed onto writeInLine, which will interpret it as a variable with single `#`
    val AttrUnstableSerdeAny = """##[cfg(all(aws_sdk_unstable, any(feature = "serde-serialize", feature = "serde-deserialize")))]"""
    val AttrUnstableSerdeBoth = """
        all(aws_sdk_unstable, all(feature = "serde-serialize", feature = "serde-deserialize"))
    """
    val AttrUnstableSerialize = """
        all(aws_sdk_unstable, feature = "serde-serialize")
    """
    val AttrUnstableDeserialize = """
        all(aws_sdk_unstable, feature = "serde-deserialize")
    """

    // double `#` is because this data is passed onto writeInLine, which will interpret it as a variable with single `#`
    val UnstableDerive = """##[cfg_attr(all(aws_sdk_unstable, feature = "serde-serialize"), derive(serde::Serialize))]
##[cfg_attr(all(aws_sdk_unstable, feature = "serde-deserialize"), derive(serde::Deserialize))]"""
}