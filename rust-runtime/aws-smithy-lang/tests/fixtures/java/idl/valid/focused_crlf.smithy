$version: "2"
namespace ex

/// Line one
/// Line two
structure S {
    a: String = """
        multi
        line
        """
    b: String = "quoted
    newline"
}
