package software.amazon.smithy.rust.codegen.util

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test

internal class StringsTest {

    @Test
    fun doubleQuote() {
        "abc".doubleQuote() shouldBe "\"abc\""
        """{"some": "json"}""".doubleQuote() shouldBe """"{\"some\": \"json\"}""""
    }
}
