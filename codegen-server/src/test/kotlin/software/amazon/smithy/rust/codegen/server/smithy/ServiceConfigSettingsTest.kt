/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ServiceConfigSettingsTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test
        use aws.protocols#restJson1
        @restJson1
        service SettingsService { operations: [Echo, Upload, PublishEvents, Stream] }
        @http(uri: "/echo", method: "POST")
        operation Echo { input := { message: String }, output := {} }
        @http(uri: "/upload", method: "POST")
        operation Upload { input := { @httpPayload data: Blob }, output := {} }
        @http(uri: "/publish", method: "POST")
        operation PublishEvents { input := { @httpPayload events: Events }, output := {} }
        @http(uri: "/stream", method: "POST")
        operation Stream { input := { @required @httpPayload data: StreamingBlob }, output := {} }
        @streaming
        blob StreamingBlob
        @streaming
        union Events { event: Event }
        structure Event { data: String }
        """.asSmithyModel()

    private fun settings(
        customization: String,
        codegen: String = "{}",
    ) = ServerRustSettings.from(
        model,
        Node.parse(
            """
            {
                "service": "test#SettingsService", "module": "settings-service",
                "moduleVersion": "0.0.1", "moduleAuthors": ["test"],
                "codegen": $codegen, "customizationConfig": $customization
            }
            """,
        ).expectObjectNode(),
    )

    @Test
    fun `protocol map matches decoupled keys and folds legacy body configuration`() {
        val inherited = settings("{}", """{"requestBodyMaxBytes": 512}""")
        check(inherited.protocolSettings()["common"]!!.getNumberMember("requestBodyMaxBytes").get().value.toLong() == 512L)
        val explicit =
            settings(
                """{"protocols":{"common":{"requestBodyMaxBytes":0},"aws.protocols#restJson1":{"customSetting":true}}}""",
                """{"requestBodyMaxBytes":512}""",
            )
        check(explicit.requestBodyMaxBytes() == 0L)
        check(explicit.protocolSettings()["aws.protocols#restJson1"]!!.getBooleanMember("customSetting").get().value)
        val copied =
            explicit.copy(
                customizationConfig = Node.parse("""{"protocols":{"common":{"requestBodyMaxBytes":2048}}}""").expectObjectNode(),
            )
        check(copied.requestBodyMaxBytes() == 2048L)
        for (key in listOf("restJson1", "aws.protocols#restJson1${'$'}member")) {
            assertThrows<CodegenException> { settings("""{"protocols":{"$key":{}}}""") }
        }
    }

    @Test
    fun `generated service configuration preserves runtime defaults and streaming exclusions`() {
        serverIntegrationTest(model) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("typed_service_configuration_defaults") {
                    rustTemplate(
                        """
                        use std::time::Duration;
                        use #{ShapeId};
                        let config = crate::service_config::service_config();
                        assert!(std::ptr::eq(config, crate::service_config::service_config()));
                        let echo = config.request_body.for_operation(&ShapeId::from_parts("test##Echo", "test", "Echo"));
                        let upload = config.request_body.for_operation(&ShapeId::from_parts("test##Upload", "test", "Upload"));
                        let stream = config.request_body.for_operation(&ShapeId::from_parts("test##Stream", "test", "Stream"));
                        let publish = config.request_body.for_operation(&ShapeId::from_parts("test##PublishEvents", "test", "PublishEvents"));
                        assert_eq!(echo.read_timeout, Some(Duration::from_secs(300)));
                        assert_eq!(upload.read_timeout, Some(Duration::from_secs(3600)));
                        assert_eq!(stream.read_timeout, None);
                        assert_eq!(publish.read_timeout, None);
                        assert_eq!(echo.max_bytes, None);
                        assert_eq!(echo.read_poll_budget.map(std::num::NonZeroUsize::get), Some(32));
                        assert_eq!(config.event_stream.global.initial_message, None);
                        assert_eq!(config.event_stream.global.message_completion, None);
                        assert_eq!(config.event_stream.global.idle, None);
                        """,
                        "ShapeId" to RuntimeType.smithySchema(context.runtimeConfig).resolve("ShapeId"),
                    )
                }
            }
        }
    }

    @Test
    fun `generated service configuration preserves explicit zero and partial event overrides`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    Node.parse(
                        """
                        {"customizationConfig":{"protocols":{"common":{
                            "requestBodyMaxBytes":128,
                            "requestBodyReadPollBudget":0,
                            "requestBodyReadTimeouts":{
                                "defaultNonPayload":"1s", "defaultPayload":"2s",
                                "perOperation":{"test#Echo":0,"test#Upload":"3s"}
                            },
                            "eventStreamTimeouts":{
                                "initialMessage":"1s", "messageCompletion":"2s", "idle":"3s",
                                "perOperation":{"test#PublishEvents":{"initialMessage":0,"messageCompletion":"4s"}}
                            }
                        }}}}
                        """,
                    ).expectObjectNode(),
            ),
        ) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("typed_service_configuration_overrides") {
                    rustTemplate(
                        """
                        use std::time::Duration;
                        use #{ShapeId};
                        let config = crate::service_config::service_config();
                        let echo = config.request_body.for_operation(&ShapeId::from_parts("test##Echo", "test", "Echo"));
                        let upload = config.request_body.for_operation(&ShapeId::from_parts("test##Upload", "test", "Upload"));
                        assert_eq!(config.request_body.global.read_timeout, Some(Duration::from_secs(1)));
                        assert_eq!(echo.read_timeout, None);
                        assert_eq!(upload.read_timeout, Some(Duration::from_secs(3)));
                        assert_eq!(echo.max_bytes.map(std::num::NonZeroUsize::get), Some(128));
                        assert_eq!(echo.read_poll_budget, None);
                        let events = config.event_stream.for_operation(&ShapeId::from_parts("test##PublishEvents", "test", "PublishEvents"));
                        assert_eq!(events.initial_message, None);
                        assert_eq!(events.message_completion, Some(Duration::from_secs(4)));
                        assert_eq!(events.idle, Some(Duration::from_secs(3)));
                        assert_eq!(config.event_stream.global.initial_message, Some(Duration::from_secs(1)));
                        assert_eq!(config.event_stream.global.message_completion, Some(Duration::from_secs(2)));
                        """,
                        "ShapeId" to RuntimeType.smithySchema(context.runtimeConfig).resolve("ShapeId"),
                    )
                }
            }
        }
    }
}
