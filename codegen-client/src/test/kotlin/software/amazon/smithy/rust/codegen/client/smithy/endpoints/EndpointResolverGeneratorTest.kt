package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.language.EndpointRuleSet

internal class EndpointResolverGeneratorTest {
    val ruleset = """
    {
      "serviceId": "minimal",
      "parameters": {
        "Region": {
          "type": "string",
          "builtIn": "AWS::Region",
          "required": true
        },
        "Bucket": {
            "type": "string",
            "required": false
        },
        "DisableHttp": {
            "type": "Boolean"
        }
      },
      "rules": [
        {
          "documentation": "base rule",
          "conditions": [
            { "fn": "isSet", "argv": [ {"ref": "DisableHttp" } ] },
            { "fn": "booleanEquals", "argv": [{"ref": "DisableHttp"}, true] }
          ],
          "endpoint": {
            "url": "{Region}.amazonaws.com",
            "authSchemes": [
              "v4"
            ],
            "authParams": {
              "v4": {
                "signingName": "serviceName",
                "signingScope": "{Region}"
              }
            }
          }
        }
      ]
    }
    """.toRuleset()

//    @Test
//    fun `generate params`() {
//        val crate = TestWorkspace.testProject()
//        val generator = EndpointResolverGenerator(ruleset, TestRuntimeConfig)
//        crate.lib {
//            it.rustTemplate(
//                """
//            fn main() {
//                let params = #{Params} {
//                    region: "foo".to_string(),
//                    bucket: None,
//                    disable_http: Some(false)
//                };
//            }
//        """, "Params" to generator.endpointParamStruct()
//            )
//        }
//        crate.compileAndTest()
//    }
//
//    @Test
//    fun `generate rules`() {
//        val crate = TestWorkspace.testProject()
//        val generator = EndpointResolverGenerator(ruleset, TestRuntimeConfig)
//        crate.lib {
//            it.rustTemplate(
//                """
//            fn main() {
//                let params = #{Params} {
//                    region: "foo".to_string(),
//                    bucket: None,
//                    disable_http: Some(false)
//                };
//                let endpoint = #{resolve_endpoint}(&params);
//            }
//        """, "Params" to generator.endpointParamStruct(), "resolve_endpoint" to generator.endpointResolver()
//            )
//        }
//        crate.compileAndTest()
//
//    }
}

fun String.toRuleset(): EndpointRuleSet {
    return EndpointRuleSet.fromNode(Node.parse(this))
}
