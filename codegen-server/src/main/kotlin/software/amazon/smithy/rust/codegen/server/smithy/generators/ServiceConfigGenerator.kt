/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/**
 * TODO Docs
 *
 * https://github.com/david-perez/smithy-rs-service-config/pull/1
 */
class ServiceConfigGenerator(
    private val codegenContext: ServerCodegenContext,
    private val pluginResolvers: List<ModelDerivedPluginsResolver>,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
    private val codegenScope =
        arrayOf(
            *RuntimeType.preludeScope,
            "Debug" to RuntimeType.Debug,
            "Display" to RuntimeType.Display,
            "HashMap" to RuntimeType.HashMap,
            "IdentityPlugin" to smithyHttpServer.resolve("plugin::IdentityPlugin"),
            "PluginPipeline" to smithyHttpServer.resolve("plugin::PluginPipeline"),
            "PluginStack" to smithyHttpServer.resolve("plugin::PluginStack"),
            "SmithyHttpServer" to smithyHttpServer,
            "StdError" to RuntimeType.StdError,
        )

    private val pluginResolverToPluginRegistrations = pluginResolvers.associateWith { it.resolve(codegenContext.model) }
    private val pluginToPluginResolvers = calculatePluginToPluginResolversMap()
    private val pluginRegistrations = pluginResolverToPluginRegistrations.flatMap { it.value }

    private fun calculatePluginToPluginResolversMap(): Map<Plugin, List<ModelDerivedPluginsResolver>> {
        val pluginToPluginResolvers = mutableMapOf<Plugin, MutableList<ModelDerivedPluginsResolver>>()
        for (pluginResolver in pluginResolvers) {
            for (pluginRegistration in pluginResolver.resolve(codegenContext.model)) {
                val pluginResolvers = pluginToPluginResolvers.getOrDefault(pluginRegistration.plugin, mutableListOf())
                pluginResolvers.add(pluginResolver)
                pluginToPluginResolvers[pluginRegistration.plugin] = pluginResolvers
            }
        }
        return pluginToPluginResolvers.toMap()
    }

    // Some checks to validate input.
    init {
        // Check that each plugin is due to a single plugin resolver.
        val pluginResolversDuplicatingPlugins = pluginToPluginResolvers.filter { it.value.size > 1 }
        if (pluginResolversDuplicatingPlugins.isNotEmpty()) {
            val msg = pluginResolversDuplicatingPlugins.map { (plugin, pluginResolvers) ->
                "- `${plugin.runtimeType.fullyQualifiedName()}`: [${pluginResolvers.joinToString(", ") { it.javaClass.name }}]"
            }.joinToString("\n")
            throw CodegenException("The following plugins are resolved by multiple plugin resolvers:\n${msg}")
        }

        // Check that each plugin is configured using a distinct setter name.
        val duplicateConfigBuilderSetterNames = pluginRegistrations
            .groupBy { it.configBuilderSetterName }
            .mapValues { (_, v) -> v.map { it.plugin } }
            .filterValues { it.size > 1 }
        if (duplicateConfigBuilderSetterNames.isNotEmpty()) {
            val msg = duplicateConfigBuilderSetterNames.map { (setter, plugins) ->
                "- `$setter`: [${plugins.joinToString(", ") { "${it.runtimeType.fullyQualifiedName()} (resolved by ${pluginToPluginResolvers[it]!!.first().javaClass.name})" }}]"
            }.joinToString("\n")
            throw CodegenException("The following plugins are configured using the same setter name:\n${msg}")
        }
    }

    private val configBuilderSetterNames = pluginRegistrations.map { it.configBuilderSetterName }.toSet()

    private fun configInsertedAtFields(): Writable =
        configBuilderSetterNames.map {
            writable { rust("${it}_inserted_at: None,") }
        }.join { "\n" }

    private fun configBuilderInsertedAtFields(): Writable =
        configBuilderSetterNames.map {
            writable { rustTemplate("pub(super) ${it}_inserted_at: #{Option}<usize>,", *codegenScope) }
        }.join { "\n" }

    private fun configBuilderCopyInsertedAtFields(insertThisOne: String?): Writable {
        if (insertThisOne != null) {
            check(configBuilderSetterNames.contains(insertThisOne))
        }

        var ret = configBuilderSetterNames.filter { it != insertThisOne }.map {
            writable { rust("${it}_inserted_at: self.${it}_inserted_at,") }
        }.join { "\n" }

        if (insertThisOne != null) {
            ret += writable {
                rust("${insertThisOne}_inserted_at: Some(self.plugin_count),")
            }
        }

        return ret
    }

    private fun configBuilderSetters(pluginRegistrations: List<PluginRegistration>): Writable =
        pluginRegistrations.map {
            val paramList = it.configBuilderSetterParams.map { (paramName, paramRuntimeType) ->
                writable {
                    rustTemplate("$paramName: #{ParamRuntimeType},", "ParamRuntimeType" to paramRuntimeType)
                }
            }.join { " " }

            // TODO Building the plugin can be fallible.
            writable {
                rustTemplate(
                    """
                    /// ${it.configBuilderSetterDocs}
                    pub fn ${it.configBuilderSetterName}(
                        self,
                        #{ParamList:W}
                    ) -> #{Result}<Builder<#{PluginStack}<#{PluginRuntimeType}, Plugin>>, Error> {
                        let built_plugin = {
                            #{PluginInstantiation:W}
                        };
                        if self.${it.configBuilderSetterName}_inserted_at.is_some() {
                            return Err(Error {
                                msg: "`${it.configBuilderSetterName}` can only be configured once".to_owned(),
                            });
                        }
                        Ok(Builder {
                            plugin_pipeline: self.plugin_pipeline.push(built_plugin),
                            #{ConfigBuilderCopyInsertedAtFields:W}
                            plugin_count: self.plugin_count + 1,
                        })
                    }
                    """,
                    *codegenScope,
                    "ParamList" to paramList,
                    "PluginRuntimeType" to it.plugin.runtimeType,
                    "ConfigBuilderCopyInsertedAtFields" to configBuilderCopyInsertedAtFields(insertThisOne = it.configBuilderSetterName),
                    "PluginInstantiation" to it.pluginInstantiation(),
                )
            }
        }.join { "\n" }

    private fun configBuilderBuildFn(topoSortedPluginRegistrations: List<PluginRegistration>): Writable = {
        if (topoSortedPluginRegistrations.isEmpty()) {
            rustTemplate(
                """
                pub fn build(self) -> super::Config<#{PluginPipeline}<Plugin>> {
                    super::Config {
                        plugin: self.plugin_pipeline,
                    }
                }
                """,
                *codegenScope,
            )
        } else {
            rustBlockTemplate(
                "pub fn build(self) -> #{Result}<super::Config<#{PluginPipeline}<Plugin>>, Error>",
                *codegenScope,
            ) {
                // First check that required plugins have been registered and return early if not.
                rust("let mut msg = String::new();")
                // TODO Unit test optional plugins are really optional.
                for (pluginRegistration in topoSortedPluginRegistrations.filter { !it.optional }) {
                    rust(
                        """
                        if self.${pluginRegistration.configBuilderSetterName}_inserted_at.is_none() {
                            msg += &format!("\n- `${pluginRegistration.configBuilderSetterName}`");
                        }
                        """,
                    )
                }
                rust(
                    """
                    if !msg.is_empty() {
                        return Err(
                            Error {
                                msg: format!("You must configure the following for `${codegenContext.serviceShape.id.name.toPascalCase()}`:{}", msg)
                            }
                        );
                    }
                    """,
                )

                // Now check plugins have been registered in the correct order.
                // We again key by `Plugin` and not by `PluginRegistration`.
                val pluginToId = topoSortedPluginRegistrations.mapIndexed { idx, p -> p.plugin to idx }.toMap()
                val adjList = topoSortedPluginRegistrations.mapIndexed { idx, p ->
                    val predecessorIds = p.predecessors.map { pluginToId[it] }.joinToString(", ")
                    writable {
                        rust(
                            """
                            Node {
                                active: true,
                                id: $idx,
                                predecessors: vec![$predecessorIds],
                            },
                            """
                        )
                    }
                }.join("")
                val settersList = topoSortedPluginRegistrations.joinToString(", ") { it.configBuilderSetterName.dq() }
                val registrations = topoSortedPluginRegistrations.map {
                    writable {
                        rust(
                            """
                            self.${it.configBuilderSetterName}_inserted_at,
                            """
                        )
                    }
                }.join("")
                rustTemplate(
                    """
                    let id_to_name: [&'static str; ${topoSortedPluginRegistrations.size}] = [$settersList];
                    
                    struct Node {
                        active: bool,
                        id: usize,
                        predecessors: Vec<usize>
                    }
                    
                    let mut adj_list: [Node; ${topoSortedPluginRegistrations.size}] = [
                        #{AdjList:W}
                    ];
                    
                    let props: [Option<usize>; ${topoSortedPluginRegistrations.size}] = [
                        #{Registrations:W}
                    ];
                    let mut registrations: Vec<(usize, usize)> = props
                        .iter()
                        .zip(0..)
                        .filter_map(|(inserted_at, id)| inserted_at.map(|idx| (idx, id)))
                        .collect();
                        registrations.sort();

                    for (_inserted_at, id) in registrations {
                        debug_assert!(adj_list[id].active);

                        let unregistered_predecessors = adj_list[id]
                            .predecessors
                            .iter()
                            .filter(|id| adj_list[**id].active)
                            .map(|id| id_to_name[*id])
                            .fold(String::new(), |mut acc, x| {
                                acc.reserve(5 + x.len());
                                acc.push_str("\n -");
                                acc.push_str("`");
                                acc.push_str(x);
                                acc.push_str("`");
                                acc
                            });
                        if !unregistered_predecessors.is_empty() {
                            return Err(Error {
                                msg: format!(
                                    "The following must be configured before `{}`:{}",
                                    id_to_name[id], unregistered_predecessors
                                ),
                            });
                        }

                        adj_list[id].active = false;
                    }
                    """,
                    *codegenScope,
                    "AdjList" to adjList,
                    "Registrations" to registrations,
                )

                // Everything is ok.
                rust(
                    """
                    Ok(super::Config {
                        plugin: self.plugin_pipeline,
                    })
                    """,
                )
            }
        }
    }

    fun render(writer: RustWriter) {
        // We want to calculate a topological sorting of the graph where an edge `(u, v)` indicates that plugin `u`
        // must be registered _before_ plugin `v`. However, we have the inverse graph, since a `PluginRegistration`
        // node contains its _predecessors_, not its successors.
        // The reversed topological sorting of a graph is a valid topological sorting of the inverse graph.
        val topoSortedPluginRegistrations = topoSort(pluginRegistrations).reversed()

        writer.rustTemplate(
            """
            ##[derive(#{Debug})]
            pub struct Config<Plugin> {
                pub(crate) plugin: Plugin,
            }
            
            impl Config<()> {
                pub fn builder() -> config::Builder<#{IdentityPlugin}> {
                    config::Builder {
                        plugin_pipeline: #{PluginPipeline}::default(),
                        #{InsertedAtFields:W}
                        plugin_count: 0,
                    }
                }
            }
            
            /// Module hosting the builder for [`Config`].
            pub mod config {
                /// Error that can occur when [`build`][Builder::build]ing the [`Builder`].`
                ##[derive(#{Debug})]
                pub struct Error {
                    msg: #{String},
                }

                impl #{Display} for Error {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        writeln!(f, "{}", self.msg)?;
                        Ok(())
                    }
                }

                impl #{StdError} for Error {}

                ##[derive(#{Debug})]
                pub struct Builder<Plugin> {
                    pub(super) plugin_pipeline: #{PluginPipeline}<Plugin>,

                    #{ConfigBuilderFields:W}

                    pub(super) plugin_count: usize,
                }
                
                impl<Plugin> Builder<Plugin> {
                    /// Apply a new [plugin](#{SmithyHttpServer}::plugin) after the ones that have already been registered.
                    pub fn plugin<NewPlugin>(
                        self,
                        plugin: NewPlugin,
                    ) -> Builder<#{PluginStack}<NewPlugin, Plugin>> {
                        Builder {
                            plugin_pipeline: self.plugin_pipeline.push(plugin),
                            #{CopyInsertedAtFields:W}
                            plugin_count: self.plugin_count + 1,
                        }
                    }
                    
                    #{ConfigBuilderSetters:W}
                    
                    #{ConfigBuilderBuildFn:W}
                }
            }
            """,
            *codegenScope,
            "InsertedAtFields" to configInsertedAtFields(),
            "ConfigBuilderFields" to configBuilderInsertedAtFields(),
            "CopyInsertedAtFields" to configBuilderCopyInsertedAtFields(insertThisOne = null),
            "ConfigBuilderSetters" to configBuilderSetters(topoSortedPluginRegistrations),
            "ConfigBuilderBuildFn" to configBuilderBuildFn(topoSortedPluginRegistrations),
        )
    }

    /**
     * Precondition: each `Plugin` is due to a single `PluginResolver`.
     * https://en.wikipedia.org/wiki/Topological_sorting#Kahn's_algorithm
     */
    private fun topoSort(pluginRegistrations: List<PluginRegistration>): List<PluginRegistration> {
        // It's a bit awkward to work with `Plugin`s instead of directly with `PluginRegistration`s, but the former
        // implements equals correctly whereas we can't put the latter directly in a map's keys because the
        // references in `predecessors` might not point directly to elements of `pluginRegistrations`!
        val pluginToPluginRegistration = pluginRegistrations.associateBy { it.plugin }
        val inDegree = pluginRegistrations.map { it.plugin }.associateWith { 0 }.toMutableMap()

        for (u in pluginRegistrations) {
            for (v in u.predecessors) {
                inDegree[v] = inDegree[v]!! + 1
            }
        }

        val q = ArrayDeque(inDegree.filterValues { it == 0 }.keys)
        var cnt = 0

        val topoSorted: MutableList<PluginRegistration> = mutableListOf()
        while (q.isNotEmpty()) {
            val u = q.removeFirst()
            val uPluginRegistration = pluginToPluginRegistration[u]!!
            topoSorted.add(uPluginRegistration)

            for (v in uPluginRegistration.predecessors) {
                inDegree[v] = inDegree[v]!! - 1

                if (inDegree[v] == 0) {
                    q.add(v)
                }
            }

            cnt += 1
        }

        if (cnt != pluginRegistrations.size) {
            // TODO Better exception message: print cycle
            throw CodegenException("Cycle detected")
        }

        return topoSorted
    }

    @JvmInline
    value class Plugin(val runtimeType: RuntimeType)

    interface PluginRegistration {
        /**
         * The actual Rust type implementing the `aws_smithy_http_server::plugin::Plugin` trait.
         */
        val plugin: Plugin

        /**
         * The list of plugins that MUST be configured before this one.
         */
        val predecessors: List<Plugin>
        val optional: Boolean

        /**
         * The setter name to register this plugin in the service config builder. It is recommended this
         * be a verb in imperative form without the `set_` prefix, e.g. `authenticate`.
         */
        val configBuilderSetterName: String

        /**
         * The Rust docs for the setter method. This should be a string not prefixed with `///`.
         */
        val configBuilderSetterDocs: String

        /**
         * The list of parameters of the setter method. Keys are variable binding names and values are Rust types.
         */
        val configBuilderSetterParams: Map<String, RuntimeType>

        /**
         * An expression to instantiate the plugin. This expression can make use of the variable binding names for
         * the parameters defined in `configBuilderSetterParams`.
         */
        fun pluginInstantiation(): Writable
    }

    interface ModelDerivedPluginsResolver {
        /**
         * Given a model, returns the set of plugins that must be registered in the service config object.
         * The service cannot be built without these plugins properly configured and applied.
         */
        fun resolve(model: Model): Set<PluginRegistration>
    }
}
