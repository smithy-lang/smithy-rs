package software.amazon.smithy.rust.codegen.core.util

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

internal class StringUtilsTest {

    @Test
    fun toRustName() {
        val input = "bucketArn.resourceId[4]"
        val output = input.toRustName()

        assertEquals(
            "bucket_arn_resource_id_4",
            output,
        )
    }
}
