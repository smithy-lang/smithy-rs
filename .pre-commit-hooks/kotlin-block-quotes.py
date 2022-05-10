#!/usr/bin/env python

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

#
# Script for pre-commit that fixes Kotlin block quote indentation
# for Smithy codegen, where the actual whitespace in the block quotes
# doesn't actually matter.
#
# In anticipation that the script isn't perfect, it will not change any
# file if non-indentation changes were made. Instead, it fails and says
# where the ambiguous code is so that it can be touched up manually.
#
# To run unit tests, run this script directly with the `--self-test` arg.
# To test against the repository, run `pre-commit run --all --verbose`.
#
import re
import sys
import unittest
from enum import Enum

INDENT_SIZE = 4

# Chops of any line comment
def without_line_comment(line):
    line_comment_start = line.find("//")
    if line_comment_start != -1:
        return line[:line_comment_start]
    return line

def _calc_block_comment(line, direction):
    regex = "(" + re.escape("/*") + "|" + re.escape("*/") + "|" + re.escape("//") + ")"
    tokens = [m.string[m.start(0):m.end(0)] for m in re.finditer(regex, line)]
    depth = 0
    for token in tokens:
        if direction > 0 and token == "//" and depth == 0:
            break
        elif token == "/*":
            depth += direction
        elif token == "*/":
            depth -= direction
    return depth > 0

# Returns True if the line starts a block comment
def starts_block_comment(line):
    return _calc_block_comment(line, 1)

# Returns True if the line ends a block comment
def ends_block_comment(line):
    return _calc_block_comment(line, -1)

# Returns True if the line starts or ends a block quote (depending on state)
def starts_or_ends_block_quote(line, inside_block_quotes):
    regex = "(" + re.escape('"""') + "|" + re.escape("//") + ")"
    tokens = [m.string[m.start(0):m.end(0)] for m in re.finditer(regex, line)]
    start_value = inside_block_quotes
    for token in tokens:
        if not inside_block_quotes and token == "//":
            break
        elif token == '"""':
            inside_block_quotes = not inside_block_quotes
    return start_value != inside_block_quotes

# Returns the indentation of a line
def line_indent(line):
    indent = re.search("[^\s]", line)
    if indent != None:
        return indent.start(0)
    else:
        return 0

# Changes the indentation of a line
def adjust_indent(line, indent):
    old_indent = re.search("[^\s]", line)
    if old_indent == None:
        return line
    line = line[old_indent.start(0):]
    return (" " * indent) + line

# Parser state.
class State(Enum):
    Default = 0 # Just started, or not inside a block comment or block quote
    InsideBlockComment = 1
    InsideBlockQuote = 2

# Fixes block quote indentation and returns a list of line numbers changed
def fix_lines(lines):
    state = State.Default
    changed = []
    correct_indent = 0
    correct_end_indent = 0
    first_inner_indent = None

    for index, line in enumerate(lines):
        # Look for block quotes or block comments
        if state == State.Default:
            if starts_block_comment(line):
                state = State.InsideBlockComment
            elif starts_or_ends_block_quote(line, inside_block_quotes = False):
                state = State.InsideBlockQuote
                correct_end_indent = line_indent(line)
                # Determine correct block comment indentation once one is found
                if line.lstrip().startswith('"""'):
                    correct_indent = line_indent(line)
                else:
                    correct_indent = line_indent(line) + INDENT_SIZE
                first_inner_indent = None

        # Skip all lines inside of block comments
        elif state == State.InsideBlockComment:
            if ends_block_comment(line):
                state = State.Default

        # Format block quotes
        elif state == State.InsideBlockQuote:
            if first_inner_indent == None and len(line.strip()) == 0:
                continue

            current_indent = line_indent(line)
            # Track the first line's indentation inside of the block quote
            # so that relative indentation can be preserved.
            if first_inner_indent == None:
                first_inner_indent = current_indent
            # Handle the end of the block quote
            if starts_or_ends_block_quote(line, inside_block_quotes = True):
                if line.lstrip().startswith('"""') and current_indent != correct_end_indent:
                    lines[index] = adjust_indent(line, correct_end_indent)
                    changed.append(index + 1)
                state = State.Default
            else:
                # Handle lines in the middle of the block quote
                indent_relative_to_first = max(0, current_indent - first_inner_indent)
                adjusted_indent = correct_indent + indent_relative_to_first
                if current_indent != adjusted_indent:
                    lines[index] = adjust_indent(line, adjusted_indent)
                    changed.append(index + 1)

    return changed

# Determines if the changes made were only to indentation
def only_changed_indentation(lines_before, lines_after):
    if len(lines_before) != len(lines_after):
        return False
    for index in range(0, len(lines_before)):
        if lines_before[index].lstrip() != lines_after[index].lstrip():
            return False
    return True

# Fixes the indentation in a file, and returns True if the file was changed
def fix_file(file_name):
    lines = []
    with open(file_name, "r") as file:
        lines = file.readlines()
    old_lines = lines[:]
    changed_line_numbers = fix_lines(lines)
    if len(changed_line_numbers) > 0 and old_lines != lines:
        # This script isn't perfect, so if anything other than whitespace changed,
        # then bail to avoid losing any code changes.
        if not only_changed_indentation(old_lines, lines):
            print("ERROR: `" + file_name + "`: Block quote indentation is wrong on lines " + str(changed_line_numbers) + \
                ". The pre-commit script can't fix it automatically in this instance.")
            sys.exit(1)
        else:
            text = "".join(lines)
            with open(file_name, "w") as file:
                file.write(text)
            print("INFO: Fixed indentation in `" + file_name + "`.")
            return True
    else:
        print("INFO: `" + file_name + "` is fine.")
        return False

class SelfTest(unittest.TestCase):
    def test_starts_block_comment(self):
        assert(not starts_block_comment(""))
        assert(not starts_block_comment("foo"))
        assert(not starts_block_comment("/* false */"))
        assert(not starts_block_comment("    /* false */"))
        assert(not starts_block_comment("    /* false */ asdf"))
        assert(not starts_block_comment("  asdf  /* false */ asdf"))
        assert(not starts_block_comment("    /* false */ /* false */"))
        assert(not starts_block_comment("    /* false /* false */ */"))
        assert(not starts_block_comment("    /* false /* false /* false */ */ */"))
        assert(not starts_block_comment("   false */"))
        assert(not starts_block_comment("/* false //*/"))
        assert(not starts_block_comment("    /* false /* false /* false */ */ // */"))
        assert(not starts_block_comment("// /* false"))
        assert(starts_block_comment("    /* true *"))
        assert(starts_block_comment("    /* true */ /*"))
        assert(starts_block_comment("    /* true /* true /* true */ */"))

    def test_ends_block_comment(self):
        assert(not ends_block_comment(""))
        assert(ends_block_comment("*/"))
        assert(ends_block_comment("// */"))
        assert(ends_block_comment("  */ asdf"))
        assert(ends_block_comment("  asdf */ asdf"))
        assert(not ends_block_comment(" /* asdf */ asdf"))
        assert(not ends_block_comment("    /* true */ /*"))
        assert(not ends_block_comment("    /* true /* true /* true */ */"))

    def test_starts_or_ends_block_quote(self):
        assert(not starts_or_ends_block_quote("", False))
        assert(not starts_or_ends_block_quote('  """foo "bar" baz"""', False))
        assert(not starts_or_ends_block_quote('  """foo "bar" baz""" test """foo"""', False))
        assert(starts_or_ends_block_quote('  """foo "bar" baz""" test """foo', False))
        assert(starts_or_ends_block_quote('"""', False))

        assert(not starts_or_ends_block_quote('// """', False))
        assert(starts_or_ends_block_quote('"""//""" """', False))
        assert(not starts_or_ends_block_quote('"""//"""', False))

        assert(starts_or_ends_block_quote('// """', True))
        assert(starts_or_ends_block_quote('"""//""" """', True))
        assert(starts_or_ends_block_quote('"""//"""', True))

    def test_line_indent(self):
        self.assertEqual(line_indent(""), 0)
        self.assertEqual(line_indent("   "), 0)
        self.assertEqual(line_indent("   foo"), 3)
        self.assertEqual(line_indent("   foo bar"), 3)

    def test_adjust_indent(self):
        self.assertEqual(adjust_indent("", 3), "")
        self.assertEqual(adjust_indent("foo", 3), "   foo")
        self.assertEqual(adjust_indent(" foo", 3), "   foo")

    def test_only_changed_indentation(self):
        assert(only_changed_indentation(["foo"], ["foo"]))
        assert(only_changed_indentation(["foo"], ["    foo"]))
        assert(not only_changed_indentation(["foo"], ["oo"]))
        assert(not only_changed_indentation(["foo"], ["foo", "bar"]))
        assert(not only_changed_indentation(["foo", "bar"], ["foo"]))
        assert(not only_changed_indentation(["  foo"], ["  oo"]))

    def fix_lines_test_case(self, expected, input, lines_changed):
        actual_lines_changed = fix_lines(input)
        self.assertEqual(expected, input)
        self.assertEqual(lines_changed, actual_lines_changed)

    def test_fix_lines(self):
        self.fix_lines_test_case( \
            expected = ['  """', '  if something {', '      foo();', '  }', '  """'], \
            input = ['  """', '  if something {', '      foo();', '  }', '"""'], \
            lines_changed = [5] \
        )
        self.fix_lines_test_case( \
            expected = ['  foo = """', '      asdf', '  """'], \
            input = ['  foo = """', '    asdf', '    """'], \
            lines_changed = [2, 3] \
        )
        self.fix_lines_test_case( \
            expected = ['  foo = """', '      // asdf', '  //"""'], \
            input = ['  foo = """', '      // asdf', '  //"""'], \
            lines_changed = [] \
        )
        self.fix_lines_test_case( \
            expected = ['    """', '    asdf {', '        asdf', '    }', '    """'], \
            input = ['    """', '  asdf {', '      asdf', '  }', '"""'], \
            lines_changed = [2, 3, 4, 5] \
        )
        self.fix_lines_test_case( \
            expected = ['    """', '', '    foo', '    bar', '    """'], \
            input = ['    """', '', '    foo', '    bar', '    """'], \
            lines_changed = [] \
        )

def main():
    # Run unit tests if given `--self-test` argument
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        sys.argv.pop()
        unittest.main()
    else:
        file_names = sys.argv[1:]
        status = 0
        for file_name in file_names:
            if fix_file(file_name):
                status = 1
        sys.exit(status)

if __name__ == "__main__":
    main()
