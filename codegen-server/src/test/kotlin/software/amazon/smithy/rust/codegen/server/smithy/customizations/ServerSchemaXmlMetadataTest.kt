/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

internal class ServerSchemaXmlMetadataTest {
    @Test
    fun `structured payload request roots retain legacy behavior`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.xmlmetadata
            use aws.protocols#restXml

            @restXml
            service Payloads {
                version: "1"
                operations: [Fallback, TargetOnly, MemberOnly, Both, Shared, UnionPayload, Nested, Namespaced, Modeled, Stream]
            }
            @http(method: "POST", uri: "/fallback")
            operation Fallback { input := { @httpPayload data: Plain }, output: Echo }
            @http(method: "POST", uri: "/target")
            operation TargetOnly { input := { @httpPayload data: Renamed }, output: Echo }
            @http(method: "POST", uri: "/member")
            operation MemberOnly { input := { @httpPayload @xmlName("MemberRoot") data: Plain }, output: Echo }
            @http(method: "POST", uri: "/both")
            operation Both { input := { @httpPayload @xmlName("MemberRoot") data: Renamed }, output: Echo }
            @http(method: "POST", uri: "/shared")
            operation Shared { input := { @httpPayload @xmlName("OtherRoot") data: Renamed }, output: Echo }
            @http(method: "POST", uri: "/union")
            operation UnionPayload { input := { @httpPayload @xmlName("UnionRoot") data: Choice }, output: Echo }
            @http(method: "POST", uri: "/nested")
            operation Nested { input := { @httpPayload @xmlName("NestedRoot") data: Parent }, output: Echo }
            @http(method: "POST", uri: "/namespaced")
            operation Namespaced {
                input := {
                    @httpPayload
                    @xmlName("NamespaceRoot")
                    @xmlNamespace(uri: "urn:member", prefix: "m")
                    data: Renamed
                }
                output: Echo
            }
            @http(method: "POST", uri: "/modeled")
            operation Modeled {
                input := { @httpPayload @xmlName("ErrorRoot") data: Plain }
                output: Echo
                errors: [Problem]
            }
            @http(method: "POST", uri: "/stream")
            operation Stream { input := { @httpPayload @required data: StreamingBlob }, output: Echo }

            structure Plain { @xmlName("Child") child: String }
            @xmlName("TargetRoot")
            @xmlNamespace(uri: "urn:target", prefix: "t")
            structure Renamed { @xmlName("Child") child: String }
            @xmlName("ChoiceTarget")
            union Choice { @xmlName("Child") child: String }
            structure Parent { @xmlName("NestedChild") child: Plain }
            @streaming
            blob StreamingBlob
            structure Echo { @httpPayload echoed: String }
            @error("client")
            @httpError(418)
            structure Problem { message: String }
            """.asSmithyModel()

        val snapshots = Files.createTempDirectory("smithy-xml-metadata-parity")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = snapshots.resolve("${if (schemaSerde) "schema" else "legacy"}.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("request_root_snapshots") {
                        rustTemplate(
                            """
                            fn plain(data: #{Option}<crate::model::Plain>) -> #{String} {
                                data.map(|v| v.child.unwrap_or_else(|| "unset".into()))
                                    .unwrap_or_else(|| "missing".into())
                            }
                            fn renamed(data: #{Option}<crate::model::Renamed>) -> #{String} {
                                data.map(|v| v.child.unwrap_or_else(|| "unset".into()))
                                    .unwrap_or_else(|| "missing".into())
                            }
                            let config = crate::service::PayloadsConfig::builder().build();
                            let service = crate::service::Payloads::builder(config)
                                .fallback(|input: crate::input::FallbackInput| async move {
                                    crate::output::FallbackOutput::builder().echoed(#{Some}(plain(input.data))).build()
                                })
                                .target_only(|input: crate::input::TargetOnlyInput| async move {
                                    crate::output::TargetOnlyOutput::builder().echoed(#{Some}(renamed(input.data))).build()
                                })
                                .member_only(|input: crate::input::MemberOnlyInput| async move {
                                    crate::output::MemberOnlyOutput::builder().echoed(#{Some}(plain(input.data))).build()
                                })
                                .both(|input: crate::input::BothInput| async move {
                                    crate::output::BothOutput::builder().echoed(#{Some}(renamed(input.data))).build()
                                })
                                .shared(|input: crate::input::SharedInput| async move {
                                    crate::output::SharedOutput::builder().echoed(#{Some}(renamed(input.data))).build()
                                })
                                .union_payload(|input: crate::input::UnionPayloadInput| async move {
                                    let value = match input.data {
                                        #{Some}(crate::model::Choice::Child(value)) => value,
                                        #{None} => "missing".into(),
                                    };
                                    crate::output::UnionPayloadOutput::builder().echoed(#{Some}(value)).build()
                                })
                                .nested(|input: crate::input::NestedInput| async move {
                                    let value = input.data.and_then(|data| data.child);
                                    crate::output::NestedOutput::builder().echoed(#{Some}(plain(value))).build()
                                })
                                .namespaced(|input: crate::input::NamespacedInput| async move {
                                    crate::output::NamespacedOutput::builder().echoed(#{Some}(renamed(input.data))).build()
                                })
                                .modeled(|_: crate::input::ModeledInput| async {
                                    #{Err}::<crate::output::ModeledOutput, crate::error::ModeledError>(
                                        crate::error::Problem::builder().message(#{Some}("modeled failure".into())).build().into()
                                    )
                                })
                                .stream(|input: crate::input::StreamInput| async move {
                                    let bytes = input.data.collect().await.unwrap().into_bytes();
                                    let value = #{String}::from_utf8(bytes.to_vec()).unwrap();
                                    #{Ok}::<_, crate::error::StreamError>(crate::output::StreamOutput::builder().echoed(#{Some}(value)).build())
                                })
                                .build_unchecked();
                            let mut snapshots = #{String}::new();
                            for (path, content_type, body) in [
                                ("/fallback", "application/xml", "<Plain><Child>fallback</Child></Plain>"),
                                ("/target", "application/xml", "<TargetRoot><Child>target</Child></TargetRoot>"),
                                ("/member", "application/xml", "<MemberRoot><Child>member</Child></MemberRoot>"),
                                ("/both", "application/xml", "<MemberRoot><Child>both</Child></MemberRoot>"),
                                ("/shared", "application/xml", "<OtherRoot><Child>shared</Child></OtherRoot>"),
                                ("/union", "application/xml", "<UnionRoot><Child>union</Child></UnionRoot>"),
                                ("/nested", "application/xml", "<NestedRoot><NestedChild><Child>nested</Child></NestedChild></NestedRoot>"),
                                ("/namespaced", "application/xml", r##"<m:NamespaceRoot xmlns:m="urn:member"><Child>namespace</Child></m:NamespaceRoot>"##),
                                ("/member", "application/xml", "<MemberRoot/>"),
                                ("/member", "application/xml", ""),
                                ("/member", "application/xml", "<Plain/>"),
                                ("/both", "application/xml", "<TargetRoot/>"),
                                ("/member", "application/xml", "<Wrong/>"),
                                ("/member", "application/xml", "<MemberRoot>"),
                                ("/modeled", "application/xml", "<ErrorRoot><Child>error</Child></ErrorRoot>"),
                                ("/stream", "application/octet-stream", "streamed"),
                            ] {
                                let request = #{Http}::Request::builder().method("POST").uri(path)
                                    .header("content-type", content_type)
                                    .body(#{Server}::body::to_boxed_sync(body)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                let status = response.status();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_string()))
                                    .collect();
                                headers.sort();
                                let bytes = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 1024 * 1024))
                                    .await.unwrap().to_bytes();
                                use std::fmt::Write;
                                writeln!(snapshots, "{path}\t{body}\t{status}\t{headers:?}\t{:?}", bytes.as_ref()).unwrap();
                            }
                            std::fs::write(${snapshot.toString().dq()}, snapshots).unwrap();
                            """,
                            *preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                        )
                    }
                }
            }
        }
        assertEquals(
            Files.readString(snapshots.resolve("legacy.txt")),
            Files.readString(snapshots.resolve("schema.txt")),
            "Request status, headers, and body must match the legacy generated server",
        )
    }

    @Test
    fun `XML metadata preserves target schemas and is ignored by JSON`() {
        for (xml in listOf(false, true)) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.xmlmetadata
                use aws.protocols#restJson1
                use aws.protocols#restXml
                @restJson1
                ${if (xml) "@restXml" else ""}
                service Example { version: "1", operations: [Upload, Other] }
                @http(method: "POST", uri: "/upload")
                operation Upload {
                    input := { @httpPayload @xmlName("PayloadRoot") data: Target }
                    output: Echo
                }
                @http(method: "POST", uri: "/other")
                operation Other {
                    input := { @httpPayload @xmlName("OtherRoot") data: Target }
                    output: Echo
                }
                @sensitive
                @xmlName("TargetRoot")
                @xmlNamespace(uri: "urn:target", prefix: "t")
                structure Target {
                    @xmlName("Child") child: String
                    schemaForXmlPayload: String
                    dataXmlPayloadSchema: String
                }
                structure Echo { @httpPayload echoed: String }
                """.asSmithyModel()
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode(),
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("metadata_and_protocol_selection") {
                        rustTemplate(
                            """
                            let member = crate::input::UploadInput::SCHEMA.members()[0];
                            let metadata = #{Metadata}::from_schema(member);
                            assert_eq!(metadata.is_some(), $xml);
                            if let #{Some}(metadata) = metadata {
                                let root = metadata.payload_schema();
                                let target = crate::model::Target::SCHEMA;
                                assert_eq!(root.shape_id(), target.shape_id());
                                assert_eq!(root.shape_type(), target.shape_type());
                                assert_eq!(root.original_name(), target.original_name());
                                assert_eq!(root.xml_name().unwrap().value(), "PayloadRoot");
                                assert_eq!(target.xml_name().unwrap().value(), "TargetRoot");
                                assert!(root.sensitive().is_some());
                                let namespace = root.xml_namespace().unwrap();
                                assert_eq!(namespace.uri(), "urn:target");
                                assert_eq!(namespace.prefix(), #{Some}("t"));
                                for (actual, expected) in root.members().iter().zip(target.members()) {
                                    assert!(std::ptr::eq(*actual, *expected));
                                }
                                let other = #{Metadata}::from_schema(crate::input::OtherInput::SCHEMA.members()[0])
                                    .unwrap().payload_schema();
                                assert_eq!(other.xml_name().unwrap().value(), "OtherRoot");
                                assert!(!std::ptr::eq(root, target));
                                assert!(!std::ptr::eq(root, other));

                                static COMPUTATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                                static CACHE: #{Schema}::extension::SchemaExtensionKey<#{String}> =
                                    #{Schema}::extension::SchemaExtensionKey::new(|schema| {
                                        COMPUTATIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                        schema.xml_name().unwrap().value().to_owned()
                                    });
                                assert_eq!(root.extension(&CACHE), "PayloadRoot");
                                let again = #{Metadata}::from_schema(member).unwrap().payload_schema();
                                assert!(std::ptr::eq(root, again));
                                assert!(std::ptr::eq(root.extension(&CACHE), again.extension(&CACHE)));
                                assert_eq!(COMPUTATIONS.load(std::sync::atomic::Ordering::SeqCst), 1);
                                assert_eq!(other.extension(&CACHE), "OtherRoot");
                                assert_eq!(target.extension(&CACHE), "TargetRoot");
                                assert_eq!(COMPUTATIONS.load(std::sync::atomic::Ordering::SeqCst), 3);
                            }
                            let service = crate::service::Example::builder(crate::service::ExampleConfig::builder().build())
                                .upload(|input: crate::input::UploadInput| async move {
                                    crate::output::UploadOutput::builder()
                                        .echoed(input.data.and_then(|data| data.child)).build()
                                })
                                .other(|input: crate::input::OtherInput| async move {
                                    crate::output::OtherOutput::builder()
                                        .echoed(input.data.and_then(|data| data.child)).build()
                                })
                                .build_unchecked();
                            let cases = [
                                ("/upload", "application/json", r##"{"child":"json"}"##, "json"),
                                ("/other", "application/json", r##"{"child":"other-json"}"##, "other-json"),
                                ("/upload", "application/xml", "<PayloadRoot><Child>xml</Child></PayloadRoot>", "xml"),
                                ("/other", "application/xml", "<OtherRoot><Child>other-xml</Child></OtherRoot>", "other-xml"),
                            ];
                            for (path, content_type, body, expected) in cases {
                                if !$xml && content_type == "application/xml" { continue; }
                                let request = #{Http}::Request::builder().method("POST").uri(path)
                                    .header("content-type", content_type)
                                    .body(#{Server}::body::to_boxed_sync(body)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                assert_eq!(response.status().as_u16(), 200);
                                let bytes = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 1024))
                                    .await.unwrap().to_bytes();
                                assert_eq!(bytes.as_ref(), expected.as_bytes());
                            }
                            """,
                            *preludeScope,
                            "Schema" to RuntimeType.smithySchema(context.runtimeConfig),
                            "Metadata" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve("schema::traits::XmlMetadata"),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                        )
                    }
                }
            }
        }
    }
}
