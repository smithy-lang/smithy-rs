package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.TestInstance
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import java.util.stream.Stream

/**
 * This is just a `PluginRegistration` that allows for predecessor plugins
 * to be added after the `PluginRegistration` object is created.
 */
class MutablePluginRegistration(private val runtimeType: RuntimeType, setterName: String? = null) :
    ServiceConfigGenerator.PluginRegistration {
    override val plugin: ServiceConfigGenerator.Plugin = ServiceConfigGenerator.Plugin(runtimeType)
    override val predecessors: MutableList<ServiceConfigGenerator.Plugin> = mutableListOf()
    override val optional: Boolean = false
    override val configBuilderSetterName: String = setterName ?: runtimeType.name.lowercase()
    override val configBuilderSetterDocs: String = "Docs for ${runtimeType.name.lowercase()}."
    override val configBuilderSetterParams: Map<String, RuntimeType> = emptyMap()

    override fun pluginInstantiation(): Writable =
        writable { rustTemplate("#{Plugin}", "Plugin" to runtimeType) }

    fun addPredecessor(plugin: ServiceConfigGenerator.Plugin) {
        predecessors.add(plugin)
    }
}

private fun invertGraph(graph: List<List<Int>>): List<List<Int>> {
    val n = graph.size
    val ret: List<MutableList<Int>> = List(n) { mutableListOf() }

    for (u in 0 until n) {
        for (v in graph[u]) {
            ret[v].add(u)
        }
    }

    return ret
}

private fun pluginResolverGenerator(graph: List<List<Int>>): ServiceConfigGenerator.ModelDerivedPluginsResolver {
    // We invert the graph so that the edge `(u, v)` indicates that `v` is a predecessor of `u` in the original graph.
    val invertedGraph = invertGraph(graph)

    val pluginRegistrations = List(invertedGraph.size) { idx ->
        MutablePluginRegistration(pluginName(idx))
    }

    for ((u, predecessors) in invertedGraph.withIndex()) {
        for (p in predecessors) {
            pluginRegistrations[u].addPredecessor(pluginRegistrations[p].plugin)
        }
    }

    return object: ServiceConfigGenerator.ModelDerivedPluginsResolver {
        override fun resolve(model: Model): Set<ServiceConfigGenerator.PluginRegistration> {
            return pluginRegistrations.toSet()
        }
    }
}

/**
 * Parses a directed graph from a string describing an adjacency list. The string comprises lines.
 * The first line contains a single integer `n`, the number of nodes in the graph, numbered from 0 to `n-1`.
 * `n` lines follow. Each line is a list of integers, separated by spaces, with the outgoing edges for the node.
 *
 * https://en.wikipedia.org/wiki/Adjacency_list
 */
private fun parseGraph(s: String): List<List<Int>> {
    val lines = s.lines()
    val n = lines.first().toInt()
    val ret: List<MutableList<Int>> = List(n) { mutableListOf() }
    for (u in 1..n) {
        val l = lines[u]
        if (l.isNotEmpty()) {
            for (v in l.split(" ").map { it.toInt() }) {
                ret[u - 1].add(v)
            }
        }
    }

    return ret
}

private fun pluginName(n: Int): RuntimeType = RuntimeType("crate::Plugin$n")

private fun writePlugins(pluginNames: List<String>, smithyHttpServer: RuntimeType): Writable = pluginNames.map {
    writable {
        rustTemplate(
            """
            ##[derive(#{Debug})]
            pub struct $it;

            impl<Ser, Op, T> #{Plugin}<Ser, Op, T> for $it {
                type Output = T;

                fn apply(&self, svc: T) -> T {
                    #{Tracing}::debug!("applying plugin $it");
                    svc
                }
            }
            """,
            "Debug" to RuntimeType.Debug,
            "Plugin" to smithyHttpServer.resolve("plugin::Plugin"),
            "Tracing" to RuntimeType.Tracing,
        )
    }
}.join("")

@TestInstance(TestInstance.Lifecycle.PER_CLASS)
class ServiceConfigGeneratorTest {
    private val model =
        """
        namespace test
        
        service MyService { }
        """.asSmithyModel()

    private fun testParameters(): Stream<Arguments> {
        val noPlugins = "0" to
            writable {
                unitTest("no_plugins_makes_config_build_infallible") {
                    rust(
                        """
                        crate::service::Config::builder().build();
                        """
                    )
                }
            }

        val graphUVW =
            """3
1 2
2
""" to
                writable {
                    unitTest("plugin_0_missing") {
                        rust(
                            """
                            let err = crate::service::Config::builder().plugin1().unwrap().build().unwrap_err();
                            let msg = format!("{}", err);
                            assert_eq!(
                                &msg,
                                "You must configure the following for `MyService`:\n- `plugin0`\n- `plugin2`\n"
                            );
                            """
                        )
                    }

                    unitTest("plugin_2_misordered") {
                        rust(
                            """
                            let err = crate::service::Config::builder()
                                .plugin0().unwrap()
                                .plugin2().unwrap()
                                .plugin1().unwrap()
                                .build().unwrap_err();
                            let msg = format!("{}", err);
                            assert_eq!(
                                &msg,
                                "The following must be configured before `plugin2`:\n -`plugin1`\n"
                            );
                            """
                        )
                    }

                    unitTest("all_ok") {
                        rust(
                            """
                            crate::service::Config::builder()
                                .plugin0().unwrap()
                                .plugin1().unwrap()
                                .plugin2().unwrap()
                                .build().unwrap();
                            """
                        )
                    }
                }

        return Stream.of(
            Arguments.of(graphUVW.first, graphUVW.second),
            Arguments.of(noPlugins.first, noPlugins.second)
        )
    }

    @ParameterizedTest(name = "(#{index}) Plugin graphs. inputGraph: {0}")
    @MethodSource("testParameters")
    fun `plugin graphs`(inputGraph: String, writable: Writable) {
        val codegenContext = serverTestCodegenContext(model)
        val project = TestWorkspace.testProject(codegenContext.symbolProvider)

        val n = inputGraph.lines().first().toInt()
        val smithyHttpServer = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
        val pluginNames = (0 until n).map { pluginName(it).name }
        project.lib {
            rustTemplate(
                """
                #{Plugins}
                """,
                "Plugins" to writePlugins(pluginNames, smithyHttpServer),
            )
        }

        project.withModule(RustModule.public("service")) {
            ServiceConfigGenerator(codegenContext, listOf(pluginResolverGenerator(parseGraph(inputGraph)))).render(this)
            project.testModule(writable)
        }

        project.compileAndTest()
    }

    @Test
    fun `cycles are detected`() {
        val codegenContext = serverTestCodegenContext(model)
        val project = TestWorkspace.testProject(codegenContext.symbolProvider)
        project.withModule(RustModule.public("service")) {
            val exception = shouldThrow<CodegenException> {
                ServiceConfigGenerator(codegenContext, listOf(pluginResolverGenerator(parseGraph(
                    """3
1
2
0
"""
                )))).render(this)
            }
            exception.message shouldBe "Cycle detected"
        }
    }

    @Test
    fun `check that each plugin is due to a single plugin resolver`() {
        val codegenContext = serverTestCodegenContext(model)
        val project = TestWorkspace.testProject(codegenContext.symbolProvider)
        project.withModule(RustModule.public("service")) {
            val exception = shouldThrow<CodegenException> {
                ServiceConfigGenerator(codegenContext, listOf(
                    pluginResolverGenerator(parseGraph("1\n")),
                    pluginResolverGenerator(parseGraph("1\n"))
                )).render(this)
            }
            exception.message shouldBe
                """The following plugins are resolved by multiple plugin resolvers:
- `crate::Plugin0`: [software.amazon.smithy.rust.codegen.server.smithy.generators.ServiceConfigGeneratorTestKt${'$'}pluginResolverGenerator${'$'}1, software.amazon.smithy.rust.codegen.server.smithy.generators.ServiceConfigGeneratorTestKt${'$'}pluginResolverGenerator${'$'}1]"""
        }
    }

    @Test
    fun `check that each plugin is configured using a distinct setter name`() {
        val codegenContext = serverTestCodegenContext(model)
        val project = TestWorkspace.testProject(codegenContext.symbolProvider)
        project.withModule(RustModule.public("service")) {
            // We use an actual class instead of an object expression because the class name appears in the exception message.
            class Foo: ServiceConfigGenerator.ModelDerivedPluginsResolver {
                override fun resolve(model: Model): Set<ServiceConfigGenerator.PluginRegistration> =
                    // Two different plugins, but with the same setter name.
                    setOf(
                        MutablePluginRegistration(pluginName(1), "duplicate_setter_name"),
                        MutablePluginRegistration(pluginName(2), "duplicate_setter_name"),
                    )
            }

            val exception = shouldThrow<CodegenException> {
                ServiceConfigGenerator(codegenContext, listOf(Foo())).render(this)
            }
            exception.message shouldBe
                """The following plugins are configured using the same setter name:
- `duplicate_setter_name`: [crate::Plugin1 (resolved by software.amazon.smithy.rust.codegen.server.smithy.generators.ServiceConfigGeneratorTest${'$'}check that each plugin is configured using a distinct setter name${'$'}1${'$'}Foo), crate::Plugin2 (resolved by software.amazon.smithy.rust.codegen.server.smithy.generators.ServiceConfigGeneratorTest${'$'}check that each plugin is configured using a distinct setter name${'$'}1${'$'}Foo)]"""
        }
    }
}
