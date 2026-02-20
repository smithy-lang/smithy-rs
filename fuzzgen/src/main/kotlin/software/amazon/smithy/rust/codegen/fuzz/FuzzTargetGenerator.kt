/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.fuzz

import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Local
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.PublicImportSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleProvider
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import java.nio.file.Path
import kotlin.io.path.name

private fun rustSettings(
    fuzzSettings: FuzzSettings,
    target: TargetCrate,
) = CoreRustSettings(
    fuzzSettings.service,
    moduleVersion = "0.1.0",
    moduleName = "fuzz-target-${target.name}",
    moduleAuthors = listOf(),
    codegenConfig = CoreCodegenConfig(),
    license = null,
    runtimeConfig = fuzzSettings.runtimeConfig,
    moduleDescription = null,
    moduleRepository = null,
)

data class FuzzTargetContext(
    val target: TargetCrate,
    val fuzzSettings: FuzzSettings,
    val rustCrate: RustCrate,
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val manifest: FileManifest,
) {
    fun finalize(): FileManifest {
        val forceWorkspace =
            mapOf(
                "workspace" to listOf("_ignored" to "_ignored").toMap(),
                "lib" to mapOf("crate-type" to listOf("cdylib")),
            )
        val rustSettings = rustSettings(fuzzSettings, target)
        rustCrate.finalize(rustSettings, model, forceWorkspace, listOf(), requireDocs = false)
        return manifest
    }
}

class FuzzTargetGenerator(private val context: FuzzTargetContext) {
    private val model = context.model
    private val serviceShape = context.model.expectShape(context.fuzzSettings.service, ServiceShape::class.java)
    private val symbolProvider = PublicImportSymbolProvider(context.symbolProvider, targetCrate().name)

    private fun targetCrate(): RuntimeType {
        val path = Path.of(context.target.relativePath).toAbsolutePath()
        return CargoDependency(
            name = path.name,
            location = Local(path.parent?.toString() ?: ""),
            `package` = context.target.targetPackage(),
        ).toType()
    }

    private val smithyFuzz = context.fuzzSettings.runtimeConfig.smithyRuntimeCrate("smithy-fuzz").toType()
    private val ctx =
        arrayOf(
            "fuzz_harness" to smithyFuzz.resolve("fuzz_harness"),
            "fuzz_service" to smithyFuzz.resolve("fuzz_service"),
            "FuzzResult" to smithyFuzz.resolve("FuzzResult"),
            "Body" to smithyFuzz.resolve("Body"),
            "http" to CargoDependency.Http1x.toType(),
            "target" to targetCrate(),
        )

    private val serviceName = context.fuzzSettings.service.name.toPascalCase()

    fun generateFuzzTarget() {
        context.rustCrate.lib {
            rustTemplate(
                """
                #{fuzz_harness}!(|tx| {
                    let config = #{target}::${serviceName}Config::builder().build();
                    #{tx_clones}
                    #{target}::$serviceName::builder::<#{Body}, _, _, _>(config)#{all_operations}.build_unchecked()
                });

                """,
                *ctx,
                "all_operations" to allOperations(),
                "tx_clones" to allTxs(),
                *preludeScope,
            )
        }
    }

    private fun operationsToImplement(): List<OperationShape> {
        val index = TopDownIndex.of(model)
        return index.getContainedOperations(serviceShape).filter { operationShape ->
            // TODO(fuzzing): consider if it is possible to support event streams
            !operationShape.isEventStream(model) &&
                // TODO(fuzzing): it should be possible to support normal streaming operations
                !(
                    operationShape.inputShape(model).hasStreamingMember(model) ||
                        operationShape.outputShape(model)
                            .hasStreamingMember(model)
                ) &&
                // TODO(fuzzing): it should be possible to work backwards from constraints to satisfy them in most cases.
                !(operationShape.outputShape(model).isDirectlyConstrained(symbolProvider))
        }.toList()
    }

    private fun allTxs(): Writable =
        writable {
            operationsToImplement().forEach { op ->
                val operationName =
                    op.contextName(serviceShape).toSnakeCase().let { RustReservedWords.escapeIfNeeded(it) }
                rust("let tx_$operationName = tx.clone();")
            }
        }

    private fun allOperations(): Writable =
        writable {
            val operations = operationsToImplement()
            operations.forEach { op ->
                val operationName =
                    op.contextName(serviceShape).toSnakeCase().let { RustReservedWords.escapeIfNeeded(it) }
                val output =
                    writable {
                        val outputSymbol = symbolProvider.toSymbol(op.outputShape(model))
                        if (op.errors.isEmpty()) {
                            rust("#T::builder().build()", outputSymbol)
                        } else {
                            rust("Ok(#T::builder().build())", outputSymbol)
                        }
                    }
                rustTemplate(
                    """
                    .$operationName(move |input: #{Input}| {
                        let tx = tx_$operationName.clone();
                        async move {
                            tx.send(format!("{:?}", input)).await.unwrap();
                            #{output}
                        }
                })""",
                    "Input" to symbolProvider.toSymbol(op.inputShape(model)),
                    "output" to output,
                    *preludeScope,
                )
            }
        }
}

fun createFuzzTarget(
    target: TargetCrate,
    baseManifest: FileManifest,
    fuzzSettings: FuzzSettings,
    model: Model,
): FuzzTargetContext {
    val newManifest = FileManifest.create(baseManifest.resolvePath(Path.of(target.name)))
    val codegenConfig = CoreCodegenConfig()
    val symbolProvider =
        SymbolVisitor(
            rustSettings(fuzzSettings, target),
            model,
            model.expectShape(fuzzSettings.service, ServiceShape::class.java),
            RustSymbolProviderConfig(
                fuzzSettings.runtimeConfig,
                renameExceptions = false,
                NullableIndex.CheckMode.SERVER,
                ServerModuleProvider,
            ),
        )
    val crate =
        RustCrate(
            newManifest,
            symbolProvider,
            codegenConfig,
            NoOpDocProvider(),
        )
    return FuzzTargetContext(
        target = target,
        fuzzSettings = fuzzSettings,
        rustCrate = crate,
        model = model,
        manifest = newManifest,
        symbolProvider = symbolProvider,
    )
}
