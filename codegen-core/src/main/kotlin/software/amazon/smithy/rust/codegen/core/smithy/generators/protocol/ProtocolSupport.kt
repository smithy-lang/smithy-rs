package software.amazon.smithy.rust.codegen.core.smithy.generators.protocol

data class ProtocolSupport(
    /* Client support */
    val requestSerialization: Boolean,
    val requestBodySerialization: Boolean,
    val responseDeserialization: Boolean,
    val errorDeserialization: Boolean,
    /* Server support */
    val requestDeserialization: Boolean,
    val requestBodyDeserialization: Boolean,
    val responseSerialization: Boolean,
    val errorSerialization: Boolean,
)
