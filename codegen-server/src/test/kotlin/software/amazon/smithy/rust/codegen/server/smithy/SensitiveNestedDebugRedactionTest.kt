/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * `@sensitive` shapes reached through lists, sets and maps must be redacted in the generated `Debug` output of
 * operation inputs, outputs, nested structures and unions.
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
        serverIntegrationTest(
            model,
            IntegrationTestParams(service = "test#SensitiveService"),
            testCoverage = HttpTestType.Default,
        ) { _, crate ->
            crate.lib {
                unitTest("nested_sensitive_values_are_redacted") {
                    rust(
                        """
                        use crate::input::DoThingInput;
                        use crate::output::DoThingOutput;
                        use std::collections::HashMap;

                        const REDACTED: &str = "*** Sensitive Data Redacted ***";

                        fn assert_redacted(debug: &str) {
                            assert!(debug.contains(REDACTED), "missing redaction: {debug}");
                            assert!(!debug.contains("leak-"), "leaked a secret: {debug}");
                            assert!(debug.contains("public-value"), "missing a public value: {debug}");
                            assert!(debug.contains("label-value"), "missing a public value: {debug}");
                        }

                        let holder = crate::model::Holder {
                            label: Some("label-value".to_owned()),
                            secrets: Some(vec!["leak-holder".to_owned()]),
                        };
                        assert_eq!(format!("{:?}", holder), "Holder { label: Some(\"label-value\"), secrets: \"*** Sensitive Data Redacted ***\" }");

                        macro_rules! populate {
                            (${'$'}ty:ident) => {
                                ${'$'}ty {
                                    public: Some("public-value".to_owned()),
                                    secret_list: Some(vec!["leak-1".to_owned()]),
                                    secret_set: Some(crate::model::SecretSet::try_from(vec!["leak-2".to_owned()]).unwrap()),
                                    key_map: Some(HashMap::from([("leak-3".to_owned(), "public-value".to_owned())])),
                                    value_map: Some(HashMap::from([("public-value".to_owned(), "leak-4".to_owned())])),
                                    nested_list: Some(vec![vec!["leak-5".to_owned()]]),
                                    map_of_lists: Some(HashMap::from([("public-value".to_owned(), vec!["leak-6".to_owned()])])),
                                    holder: Some(holder.clone()),
                                    choice: Some(crate::model::Choice::Secrets(vec!["leak-7".to_owned()])),
                                }
                            };
                        }

                        assert_redacted(&format!("{:?}", populate!(DoThingInput)));
                        assert_redacted(&format!("{:?}", populate!(DoThingOutput)));

                        assert_eq!(format!("{:?}", crate::model::Choice::Secrets(vec!["leak-8".to_owned()])), REDACTED);
                        assert_eq!(format!("{:?}", crate::model::Choice::Plain("plain-value".to_owned())), "Plain(\"plain-value\")");
                        """,
                    )
                }
            }
        }
    }
}
