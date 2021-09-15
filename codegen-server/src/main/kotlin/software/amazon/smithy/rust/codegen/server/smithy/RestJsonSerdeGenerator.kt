package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolContentTypes

class RestJsonSerdeGenerator(private val protocolConfig: ProtocolConfig) {
    private val serializerGenerator: RestJsonServerSerializerGenerator
    private val deserializerGenerator: RestJsonDeserializerGenerator
    private val httpSerializerGenerator: HttpSerializerGenerator
    private val httpDeserializerGenerator: HttpDeserializerGenerator

    init {
        val httpBindingResolver =
                HttpTraitHttpBindingResolver(
                        protocolConfig.model,
                        ProtocolContentTypes.consistent("application/json"),
                )
        serializerGenerator = RestJsonServerSerializerGenerator(protocolConfig, httpBindingResolver)
        deserializerGenerator = RestJsonDeserializerGenerator(protocolConfig, httpBindingResolver)
        httpSerializerGenerator = HttpSerializerGenerator(protocolConfig, httpBindingResolver)
        httpDeserializerGenerator = HttpDeserializerGenerator(protocolConfig, httpBindingResolver)
    }

    inner class Visitor(private val writer: RustWriter) : ShapeVisitor.Default<Unit>() {
        override fun getDefault(shape: Shape?) = Unit

        override fun operationShape(shape: OperationShape?) {
            shape?.let {
                httpDeserializerGenerator.render(writer, it)
                httpSerializerGenerator.render(writer, it)
                serializerGenerator.render(writer, it)
                deserializerGenerator.render(writer, it)
            }
        }
    }

    fun render(writer: RustWriter) {
        renderSerdeError(writer)
        val visitor = Visitor(writer)
        Walker(protocolConfig.model).walkShapes(protocolConfig.serviceShape).forEach {
            it.accept(visitor)
        }
    }

    private fun renderSerdeError(writer: RustWriter) {
        writer.rust(
                """
                ##[derive(Debug)]
                pub enum Error {
                    Generic(std::borrow::Cow<'static, str>),
                    DeserializeJson(smithy_json::deserialize::Error),
                    DeserializeHeader(smithy_http::header::ParseError),
                    DeserializeLabel(std::string::String),
                    BuildInput(smithy_http::operation::BuildError),
                    BuildResponse(http::Error),
                }
                
                impl Error {
                    ##[allow(dead_code)]
                    fn generic(msg: &'static str) -> Self {
                        Self::Generic(msg.into())
                    }
                }
                
                impl From<smithy_json::deserialize::Error> for Error {
                    fn from(err: smithy_json::deserialize::Error) -> Self {
                        Self::DeserializeJson(err)
                    }
                }
                
                impl From<smithy_http::header::ParseError> for Error {
                    fn from(err: smithy_http::header::ParseError) -> Self {
                        Self::DeserializeHeader(err)
                    }
                }
                
                impl From<smithy_http::operation::BuildError> for Error {
                    fn from(err: smithy_http::operation::BuildError) -> Self {
                        Self::BuildInput(err)
                    }
                }
                                
                impl std::fmt::Display for Error {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        match *self {
                            Self::Generic(ref msg) => write!(f, "serde error: {}", msg),
                            Self::DeserializeJson(ref err) => write!(f, "json parse error: {}", err),
                            Self::DeserializeHeader(ref err) => write!(f, "header parse error: {}", err),
                            Self::DeserializeLabel(ref msg) => write!(f, "label parse error: {}", msg),
                            Self::BuildInput(ref err) => write!(f, "json payload error: {}", err),
                            Self::BuildResponse(ref err) => write!(f, "http response error: {}", err),
                        }
                    }
                }
                
                impl std::error::Error for Error {}
            """.trimIndent()
        )
    }
}
