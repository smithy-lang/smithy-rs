package software.amazon.smithy.rust.codegen.lang

data class RustModule(val name: String, val meta: Meta) {
    fun render(writer: RustWriter) {
        meta.render(writer)
        writer.write("mod $name;")
    }
}
