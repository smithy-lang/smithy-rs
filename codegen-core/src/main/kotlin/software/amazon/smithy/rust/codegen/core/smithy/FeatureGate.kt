// / this object holds constants for feature gate
object FeatureGate {
    val AwsSdkUnstable = "aws_sdk_unstable"

    // double `#` is because this data is passed onto writeInLine, which will interpret it as a variable with single `#`
    val AttrUnstableSerdeAny = """
        ##[cfg(all($AwsSdkUnstable, any(feature = "serialize", feature = "deserialize")))]
    """
    val AttrUnstableSerdeBoth = """
        all($AwsSdkUnstable, all(feature = "serialize", feature = "deserialize"))
    """
    val AttrUnstableSerialize = """
        all($AwsSdkUnstable, feature = "serialize")
    """
    val AttrUnstableDeserialize = """
        all($AwsSdkUnstable, feature = "deserialize")
    """

    // double `#` is because this data is passed onto writeInLine, which will interpret it as a variable with single `#`
    val UnstableDerive = """##[cfg_attr(all($AwsSdkUnstable, feature = "serialize"), derive(serde::Serialize))]
##[cfg_attr(all($AwsSdkUnstable, feature = "deserialize"), derive(serde::Deserialize))]"""
}
