package software.amazon.smithy.rust.codegen.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.testSymbolProvider

internal class SerializerBuilderTest {
    private val model = """
    namespace test
    structure S {
        ts: Timestamp,
        s: String,
        b: Blob
    }
    """.asSmithyModel()
    private val provider = testSymbolProvider(model)

    @Test
    fun `generate correct function names`() {
        val serializerBuilder = SerializerBuilder(provider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        serializerBuilder.serializerFor(model.lookup("test#S\$ts"))!!.name shouldBe "stdoptionoptioninstant_epoch_seconds_ser"
        serializerBuilder.serializerFor(model.lookup("test#S\$b"))!!.name shouldBe "stdoptionoptionblob_ser"
        serializerBuilder.deserializerFor(model.lookup("test#S\$b"))!!.name shouldBe "stdoptionoptionblob_deser"
        serializerBuilder.deserializerFor(model.lookup("test#S\$s")) shouldBe null
    }
}
