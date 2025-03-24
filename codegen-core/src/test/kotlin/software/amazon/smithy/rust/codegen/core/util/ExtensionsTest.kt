package software.amazon.smithy.rust.codegen.core.util

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class ExtensionsTest {
    @Test
    fun `it should find event streams on normal operations`() {
        val model =
            """
            namespace test
            service TestService {
                operations: [ EventStreamOp ]
            }

            operation EventStreamOp {
                input := {
                    events: Events
                }
            }
            
            @streaming 
            union Events {
                foo: Foo
                bar: Bar,
            }
            
            structure Foo {
                foo: String
            }
            
            structure Bar {
                bar: Long
            }
            """.asSmithyModel(smithyVersion = "2.0")

        val service = model.expectShape(ShapeId.from("test#TestService"), ServiceShape::class.java)
        service.hasEventStreamOperations(model) shouldBe true
    }

    @Test
    fun `it should find event streams on resource operations`() {
        val model =
            """
            namespace test
            service TestService {
                resources: [ TestResource ]
            }
            
            resource TestResource {
                operations: [ EventStreamOp ]
            }

            operation EventStreamOp {
                input := {
                    events: Events
                }
            }
            
            @streaming 
            union Events {
                foo: Foo
                bar: Bar,
            }
            
            structure Foo {
                foo: String
            }
            
            structure Bar {
                bar: Long
            }
            """.asSmithyModel(smithyVersion = "2.0")

        val service = model.expectShape(ShapeId.from("test#TestService"), ServiceShape::class.java)
        service.hasEventStreamOperations(model) shouldBe true
    }
}
