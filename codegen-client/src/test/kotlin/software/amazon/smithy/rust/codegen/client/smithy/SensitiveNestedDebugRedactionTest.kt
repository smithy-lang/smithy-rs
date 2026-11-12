/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

/**
 * `@sensitive` shapes reached through lists, sets and maps must be redacted in the generated `Debug` output of
 * operation inputs, outputs, nested structures, unions and their builders.
 */
class SensitiveNestedDebugRedactionTest {
    private val model =
        """
        namespace test

        use aws.protocols#restJson1
        use smithy.framework#ValidationException

        @restJson1
        service SensitiveService {
            version: "1",
            operations: [DoThing],
        }

        @http(uri: "/do-thing", method: "POST")
        operation DoThing {
            input: DoThingInput,
            output: DoThingOutput,
            errors: [ValidationException],
        }

        @sensitive
        string Secret

        list SecretList { member: Secret }

        @uniqueItems
        list SecretSet { member: Secret }

        map SecretKeyMap { key: Secret, value: String }

        map SecretValueMap { key: String, value: Secret }

        list NestedSecretList { member: SecretList }

        map MapOfSecretLists { key: String, value: SecretList }

        structure Holder {
            label: String,
            secrets: SecretList,
        }

        union Choice {
            plain: String,
            secrets: SecretList,
        }

        @mixin
        structure SecretsMixin {
            public: String,
            secretList: SecretList,
            secretSet: SecretSet,
            keyMap: SecretKeyMap,
            valueMap: SecretValueMap,
            nestedList: NestedSecretList,
            mapOfLists: MapOfSecretLists,
            holder: Holder,
            choice: Choice,
        }

        @input
        structure DoThingInput with [SecretsMixin] {}

        @output
        structure DoThingOutput with [SecretsMixin] {}
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `debug output redacts sensitive shapes nested in collections`() {
        clientIntegrationTest(model) { _, crate ->
            crate.lib {
                unitTest("nested_sensitive_values_are_redacted") {
                    rust(
                        """
                        use std::collections::HashMap;

                        const REDACTED: &str = "*** Sensitive Data Redacted ***";

                        fn assert_redacted(debug: &str) {
                            assert!(debug.contains(REDACTED), "missing redaction: {debug}");
                            assert!(!debug.contains("leak-"), "leaked a secret: {debug}");
                            assert!(debug.contains("public-value"), "missing a public value: {debug}");
                            assert!(debug.contains("label-value"), "missing a public value: {debug}");
                        }

                        let holder = crate::types::Holder::builder()
                            .label("label-value")
                            .set_secrets(Some(vec!["leak-holder".to_owned()]))
                            .build();
                        let holder_debug = format!("{:?}", holder);
                        assert_eq!(holder_debug, "Holder { label: Some(\"label-value\"), secrets: \"*** Sensitive Data Redacted ***\" }");

                        macro_rules! populate {
                            (${'$'}builder:expr) => {
                                ${'$'}builder
                                    .public("public-value")
                                    .set_secret_list(Some(vec!["leak-1".to_owned()]))
                                    .set_secret_set(Some(vec!["leak-2".to_owned()]))
                                    .set_key_map(Some(HashMap::from([("leak-3".to_owned(), "public-value".to_owned())])))
                                    .set_value_map(Some(HashMap::from([("public-value".to_owned(), "leak-4".to_owned())])))
                                    .set_nested_list(Some(vec![vec!["leak-5".to_owned()]]))
                                    .set_map_of_lists(Some(HashMap::from([("public-value".to_owned(), vec!["leak-6".to_owned()])])))
                                    .holder(holder.clone())
                                    .choice(crate::types::Choice::Secrets(vec!["leak-7".to_owned()]))
                            };
                        }

                        let input_builder = populate!(crate::operation::do_thing::DoThingInput::builder());
                        assert_redacted(&format!("{:?}", input_builder));
                        let input = input_builder.build().unwrap();
                        assert_redacted(&format!("{:?}", input));

                        let output_builder = populate!(crate::operation::do_thing::DoThingOutput::builder());
                        assert_redacted(&format!("{:?}", output_builder));
                        let output = output_builder.build();
                        assert_redacted(&format!("{:?}", output));

                        assert_eq!(format!("{:?}", crate::types::Choice::Secrets(vec!["leak-8".to_owned()])), REDACTED);
                        assert_eq!(format!("{:?}", crate::types::Choice::Plain("plain-value".to_owned())), "Plain(\"plain-value\")");
                        """,
                    )
                }
            }
        }
    }
}
