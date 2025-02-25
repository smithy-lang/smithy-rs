/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

object SupportStructures {
    private val supportModule = SerdeModule

    private val serde =
        CargoDependency.Serde.copy(
            scope = DependencyScope.Compile,
            optional = true,
            // remove `derive`
            features = setOf(),
        ).toType()

    val codegenScope =
        arrayOf(
            *RuntimeType.preludeScope,
            "ConfigurableSerde" to configurableSerde(),
            "SerializeConfigured" to serializeConfigured(),
            "ConfigurableSerdeRef" to configurableSerdeRef(),
            "SerializationSettings" to serializationSettings(),
            "Sensitive" to sensitive(),
            "serde" to serde,
            "serialize_redacted" to serializeRedacted(),
            "serialize_unredacted" to serializeUnredacted(),
        )

    fun serializeRedacted(): RuntimeType =
        RuntimeType.forInlineFun("serialize_redacted", supportModule) {
            rustTemplate(
                """
                /// Serialize a value redacting sensitive fields
                ///
                /// This function is intended to be used by `serde(serialize_with = "serialize_redacted")`
                pub fn serialize_redacted<T, S: #{serde}::Serializer>(value: &T, serializer: S) -> #{Result}<S::Ok, S::Error>
                where
                    T: #{SerializeConfigured},
                {
                    use #{serde}::Serialize;
                    value
                        .serialize_ref(&#{SerializationSettings}::redact_sensitive_fields())
                        .serialize(serializer)
                }
                """,
                "serde" to serde,
                "SerializeConfigured" to serializeConfigured(),
                "SerializationSettings" to serializationSettings(),
                *codegenScope,
            )
        }

    fun serializeUnredacted(): RuntimeType =
        RuntimeType.forInlineFun("serialize_unredacted", supportModule) {
            rustTemplate(
                """
                /// Serialize a value without redacting sensitive fields
                ///
                /// This function is intended to be used by `serde(serialize_with = "serialize_unredacted")`
                pub fn serialize_unredacted<T, S: #{serde}::Serializer>(value: &T, serializer: S) -> #{Result}<S::Ok, S::Error>
                where
                    T: #{SerializeConfigured},
                {
                    use #{serde}::Serialize;
                    value
                        .serialize_ref(&#{SerializationSettings}::leak_sensitive_fields())
                        .serialize(serializer)
                }
                """,
                "serde" to serde,
                "SerializeConfigured" to serializeConfigured(),
                "SerializationSettings" to serializationSettings(),
                *codegenScope,
            )
        }

    private fun serializeConfigured(): RuntimeType =
        RuntimeType.forInlineFun("SerializeConfigured", supportModule) {
            rustTemplate(
                """
                /// Trait that allows configuring serialization
                /// **This trait should not be implemented directly!** Instead, `impl Serialize for ConfigurableSerdeRef<T>`
                pub trait SerializeConfigured {
                    /// Return a `Serialize` implementation for this object that owns the object.
                    ///
                    /// Use this if you need to create `Arc<dyn Serialize>` or similar.
                    fn serialize_owned(self, settings: #{SerializationSettings}) -> impl #{serde}::Serialize;

                    /// Return a `Serialize` implementation for this object that borrows from the given object
                    fn serialize_ref<'a>(&'a self, settings: &'a #{SerializationSettings}) -> impl #{serde}::Serialize + 'a;
                }

                /// Blanket implementation for all `T` that implement `ConfigurableSerdeRef`
                impl<T> SerializeConfigured for T
                where
                    for<'a> #{ConfigurableSerdeRef}<'a, T>: #{serde}::Serialize,
                {
                    fn serialize_owned(
                        self,
                        settings: #{SerializationSettings},
                    ) -> impl #{serde}::Serialize {
                        #{ConfigurableSerde} {
                            value: self,
                            settings,
                        }
                    }

                    fn serialize_ref<'a>(
                        &'a self,
                        settings: &'a #{SerializationSettings},
                    ) -> impl #{serde}::Serialize + 'a {
                        #{ConfigurableSerdeRef} { value: self, settings }
                    }
                }
                """,
                "ConfigurableSerde" to configurableSerde(),
                "ConfigurableSerdeRef" to configurableSerdeRef(),
                "SerializationSettings" to serializationSettings(),
                "serde" to serde,
            )
        }

    private fun sensitive() =
        RuntimeType.forInlineFun("Sensitive", supportModule) {
            rustTemplate(
                """
                pub(crate) struct Sensitive<T>(pub(crate) T);

                impl<'a, T> #{serde}::Serialize for ConfigurableSerdeRef<'a, Sensitive<T>>
                where
                T: #{serde}::Serialize,
                {
                    fn serialize<S>(&self, serializer: S) -> #{Result}<S::Ok, S::Error>
                    where
                    S: serde::Serializer,
                    {
                        match self.settings.redact_sensitive_fields {
                            true => serializer.serialize_str("<redacted>"),
                            false => self.value.0.serialize(serializer),
                        }
                    }
                }
                """,
                "serde" to CargoDependency.Serde.toType(),
                *codegenScope,
            )
        }

    private fun configurableSerde() =
        RuntimeType.forInlineFun("ConfigurableSerde", supportModule) {
            rustTemplate(
                """
                ##[allow(missing_docs)]
                pub(crate) struct ConfigurableSerde<T> {
                    pub(crate) value: T,
                    pub(crate) settings: #{SerializationSettings}
                }

                impl<T> #{serde}::Serialize for ConfigurableSerde<T> where for <'a> ConfigurableSerdeRef<'a, T>: #{serde}::Serialize {
                    fn serialize<S>(&self, serializer: S) -> #{Result}<S::Ok, S::Error>
                    where
                        S: #{serde}::Serializer,
                    {
                        #{ConfigurableSerdeRef} {
                            value: &self.value,
                            settings: &self.settings,
                        }
                        .serialize(serializer)
                    }
                }
                impl<'a, T> #{serde}::Serialize for #{ConfigurableSerdeRef}<'a, Option<T>>
                where
                    T: #{SerializeConfigured},
                {
                    fn serialize<S>(&self, serializer: S) -> #{Result}<S::Ok, S::Error>
                    where
                        S: #{serde}::Serializer,
                    {
                        match self.value {
                            Some(value) => serializer.serialize_some(&value.serialize_ref(self.settings)),
                            None => serializer.serialize_none(),
                        }
                    }
                }

                """,
                "SerializationSettings" to serializationSettings(),
                "ConfigurableSerdeRef" to configurableSerdeRef(),
                "SerializeConfigured" to serializeConfigured(),
                "serde" to CargoDependency.Serde.toType(),
                *codegenScope,
            )
        }

    private fun configurableSerdeRef() =
        RuntimeType.forInlineFun("ConfigurableSerdeRef", supportModule) {
            rustTemplate(
                """
                ##[allow(missing_docs)]
                pub(crate) struct ConfigurableSerdeRef<'a, T> {
                    pub(crate) value: &'a T,
                    pub(crate) settings: &'a SerializationSettings
                }
                """,
            )
        }

    private fun serializationSettings() =
        RuntimeType.forInlineFun("SerializationSettings", supportModule) {
            rustTemplate(
                """
                /// Settings for use when serializing structures
                ##[non_exhaustive]
                ##[derive(Clone, Debug, Default)]
                pub struct SerializationSettings {
                    /// Replace all sensitive fields with `<redacted>` during serialization
                    pub redact_sensitive_fields: bool,

                    /// Serialize Nan, infinity and negative infinity as strings.
                    ///
                    /// For protocols like JSON, this avoids the loss-of-information that occurs when these out-of-range values
                    /// are serialized as null.
                    pub out_of_range_floats_as_strings: bool,
                }

                impl SerializationSettings {
                    /// Replace all `@sensitive` fields with `<redacted>` when serializing.
                    ///
                    /// Note: This may alter the type of the serialized output and make it impossible to deserialize as
                    /// numerical fields will be replaced with strings.
                    pub const fn redact_sensitive_fields() -> Self { Self { redact_sensitive_fields: true, out_of_range_floats_as_strings: false } }

                    /// Preserve the contents of sensitive fields during serializing
                    pub const fn leak_sensitive_fields() -> Self { Self { redact_sensitive_fields: false, out_of_range_floats_as_strings: false } }
                }
                """,
            )
        }
}
