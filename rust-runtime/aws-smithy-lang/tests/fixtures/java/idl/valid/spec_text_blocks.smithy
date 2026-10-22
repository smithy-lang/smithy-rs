$version: "2"

metadata html = """
    <div>
        <p>Hello!</p>
    </div>
    """
metadata htmlNoTrailingNewline = """
    <div>
        <p>Hello!</p>
    </div>"""
metadata incidental = """
    Foo
        Baz


    Bar
    """
metadata significantTrailingLine = """
    Foo
        Baz
    Bar
"""
metadata delimiterRightOfContent = """
    Foo
        Baz
    Bar
        """
metadata quotes = """
"hello!"
"""
metadata escapedDelimiter = """
foo \"""
baz"""
metadata escapesAfterIndentation = """
  <div>
    <p>Hi\n    bar</p>
  </div>
  """
metadata escapedNewlines = """
Foo \
Baz \
Bam"""
metadata mixedNewlines = """
Foo
Baz \
Bam"""
