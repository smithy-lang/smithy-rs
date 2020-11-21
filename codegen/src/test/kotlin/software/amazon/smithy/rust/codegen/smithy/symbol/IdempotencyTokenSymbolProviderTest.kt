package software.amazon.smithy.rust.codegen.smithy.symbol

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.asSmithy
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider

internal class IdempotencyTokenSymbolProviderTest {

    @Test
    fun `set the correct default for idempotency tokens`() {
        val model = """
        namespace smithy.example

        structure Input {
            @idempotencyToken
            member: String,

            anotherMember: String
        }

        string NotIdempotent
        """.asSmithy()
        val provider = testSymbolProvider(model)
        val struct = model.lookup<MemberShape>("smithy.example#Input\$member")
        val keySymbol = provider.toSymbol(struct)
        (keySymbol.defaultValue() is Default.Custom) shouldBe true
        val anotherKeySymbol = provider.toSymbol(model.lookup("smithy.example#Input\$anotherMember"))
        anotherKeySymbol.defaultValue() shouldBe Default.NoDefault
    }

    @Test
    fun `idempotency integration test`() {
        val model = """
        namespace smithy.example

        structure Input {
            @idempotencyToken
            member: String,

            anotherMember: String
        }
        """.asSmithy()
        val writer = RustWriter.forModule("model")
        StructureGenerator(model, testSymbolProvider(model), writer, model.lookup("smithy.example#Input")).render()
        writer.compileAndTest(
            """
            let input = Input::builder().build();
            assert_ne!(input.member, None);
            let generated_uuid = input.member.as_ref().unwrap();
            assert_eq!(generated_uuid.len(), 36);
            assert_eq!(generated_uuid.as_bytes()[8] as char, '-');

            let second_input = Input::builder().build();
            // Assert we're generating random tokens
            assert_ne!(&input.member, &second_input.member);
        """
        )
    }
}
