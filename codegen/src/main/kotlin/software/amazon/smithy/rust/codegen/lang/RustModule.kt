package software.amazon.smithy.rust.codegen.lang

data class RustModule(val name: String, val rustMetadata: RustMetadata) {
    fun render(writer: RustWriter) {
        rustMetadata.render(writer)
        writer.write("mod $name;")
    }
}
