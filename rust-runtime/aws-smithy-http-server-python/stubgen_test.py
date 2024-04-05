#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import sys
import unittest
from types import ModuleType
from textwrap import dedent
from pathlib import Path
from tempfile import TemporaryDirectory

from stubgen import Writer, walk_module


def create_module(name: str, code: str) -> ModuleType:
    mod = ModuleType(name)
    exec(dedent(code), mod.__dict__)
    if not hasattr(mod, "__all__"):
        # Manually populate `__all__` with all the members that doesn't start with `__`
        mod.__all__ = [k for k in mod.__dict__.keys() if not k.startswith("__")]  # type: ignore
    sys.modules[name] = mod
    return mod


class TestStubgen(unittest.TestCase):
    def test_function_without_docstring(self):
        self.single_mod(
            """
            def foo():
                pass
            """,
            """
            import typing

            foo: typing.Any
            """,
        )

    def test_regular_function(self):
        self.single_mod(
            """
            def foo(bar):
                '''
                :param bar str:
                :rtype bool:
                '''
                pass
            """,
            """
            def foo(bar: str) -> bool:
                ...
            """,
        )

    def test_function_with_default_value(self):
        self.single_mod(
            """
            def foo(bar, qux=None):
                '''
                :param bar int:
                :param qux typing.Optional[str]:
                :rtype None:
                '''
                pass
            """,
            """
            import typing

            def foo(bar: int, qux: typing.Optional[str] = ...) -> None:
                ...
            """,
        )

    def test_empty_class(self):
        self.single_mod(
            """
            class Foo:
                pass
            """,
            """
            class Foo:
                ...
            """,
        )

    def test_class(self):
        self.single_mod(
            """
            class Foo:
                @property
                def bar(self):
                    '''
                    :type typing.List[bool]:
                    '''
                    pass

                def qux(self, a, b, c):
                    '''
                    :param a typing.Dict[typing.List[int]]:
                    :param b str:
                    :param c float:
                    :rtype typing.Union[int, str, bool]:
                    '''
                    pass
            """,
            """
            import typing

            class Foo:
                bar: typing.List[bool]

                def qux(self, a: typing.Dict[typing.List[int]], b: str, c: float) -> typing.Union[int, str, bool]:
                    ...
            """,
        )

    def test_class_with_constructor_signature(self):
        self.single_mod(
            """
            class Foo:
                '''
                :param bar str:
                :rtype None:
                '''
            """,
            """
            class Foo:
                def __init__(self, bar: str) -> None:
                    ...
            """,
        )

    def test_class_with_static_method(self):
        self.single_mod(
            """
            class Foo:
                @staticmethod
                def bar(name):
                    '''
                    :param name str:
                    :rtype typing.List[bool]:
                    '''
                    pass
            """,
            """
            import typing

            class Foo:
                @staticmethod
                def bar(name: str) -> typing.List[bool]:
                    ...
            """,
        )

    def test_class_with_an_undocumented_descriptor(self):
        self.single_mod(
            """
            class Foo:
                @property
                def bar(self):
                    pass
            """,
            """
            import typing

            class Foo:
                bar: typing.Any
            """,
        )

    def test_enum(self):
        self.single_mod(
            """
            class Foo:
                def __init__(self, name):
                    pass

            Foo.Bar = Foo("Bar")
            Foo.Baz = Foo("Baz")
            Foo.Qux = Foo("Qux")
            """,
            """
            class Foo:
                Bar: Foo

                Baz: Foo

                Qux: Foo
            """,
        )

    def test_generic(self):
        self.single_mod(
            """
            class Foo:
                '''
                :generic T:
                :generic U:
                :extends typing.Generic[T]:
                :extends typing.Generic[U]:
                '''

                @property
                def bar(self):
                    '''
                    :type typing.Tuple[T, U]:
                    '''
                    pass

                def baz(self, a):
                    '''
                    :param a U:
                    :rtype T:
                    '''
                    pass
            """,
            """
            import typing

            T = typing.TypeVar('T')
            U = typing.TypeVar('U')

            class Foo(typing.Generic[T], typing.Generic[U]):
                bar: typing.Tuple[T, U]

                def baz(self, a: U) -> T:
                    ...
            """,
        )

    def test_items_with_docstrings(self):
        self.single_mod(
            """
            class Foo:
                '''
                This is the docstring of Foo.

                And it has multiple lines.

                :generic T:
                :extends typing.Generic[T]:
                :param member T:
                '''

                @property
                def bar(self):
                    '''
                    This is the docstring of property `bar`.

                    :type typing.Optional[T]:
                    '''
                    pass

                def baz(self, t):
                    '''
                    This is the docstring of method `baz`.
                    :param t T:
                    :rtype T:
                    '''
                    pass
            """,
            '''
            import typing

            T = typing.TypeVar('T')

            class Foo(typing.Generic[T]):
                """
                This is the docstring of Foo.

                And it has multiple lines.
                """

                bar: typing.Optional[T]
                """
                This is the docstring of property `bar`.
                """

                def baz(self, t: T) -> T:
                    """
                    This is the docstring of method `baz`.
                    """
                    ...


                def __init__(self, member: T) -> None:
                    ...
            ''',
        )

    def test_adds_default_to_optional_types(self):
        # Since PyO3 provides `impl FromPyObject for Option<T>` and maps Python `None` to Rust `None`,
        # you don't have to pass `None` explicitly. Type-stubs also shoudln't require `None`s
        # to be passed explicitly (meaning they should have a default value).

        self.single_mod(
            """
            def foo(bar, qux):
                '''
                :param bar typing.Optional[int]:
                :param qux typing.List[typing.Optional[int]]:
                :rtype int:
                '''
                pass
            """,
            """
            import typing

            def foo(bar: typing.Optional[int] = ..., qux: typing.List[typing.Optional[int]]) -> int:
                ...
            """,
        )

    def test_multiple_mods(self):
        create_module(
            "foo.bar",
            """
            class Bar:
                '''
                :param qux str:
                :rtype None:
                '''
                pass
            """,
        )

        foo = create_module(
            "foo",
            """
            import sys

            bar = sys.modules["foo.bar"]

            class Foo:
                '''
                :param a __root_module_name__.bar.Bar:
                :param b typing.Optional[__root_module_name__.bar.Bar]:
                :rtype None:
                '''

                @property
                def a(self):
                    '''
                    :type __root_module_name__.bar.Bar:
                    '''
                    pass

                @property
                def b(self):
                    '''
                    :type typing.Optional[__root_module_name__.bar.Bar]:
                    '''
                    pass

            __all__ = ["bar", "Foo"]
            """,
        )

        with TemporaryDirectory() as temp_dir:
            foo_path = Path(temp_dir) / "foo.pyi"
            bar_path = Path(temp_dir) / "bar" / "__init__.pyi"

            writer = Writer(foo_path, "foo")
            walk_module(writer, foo)
            writer.dump()

            self.assert_stub(
                foo_path,
                """
                import foo.bar
                import typing

                class Foo:
                    a: foo.bar.Bar

                    b: typing.Optional[foo.bar.Bar]

                    def __init__(self, a: foo.bar.Bar, b: typing.Optional[foo.bar.Bar] = ...) -> None:
                        ...
                """,
            )

            self.assert_stub(
                bar_path,
                """
                class Bar:
                    def __init__(self, qux: str) -> None:
                        ...
                """,
            )

    def single_mod(self, mod_code: str, expected_stub: str) -> None:
        with TemporaryDirectory() as temp_dir:
            mod = create_module("test", mod_code)
            path = Path(temp_dir) / "test.pyi"

            writer = Writer(path, "test")
            walk_module(writer, mod)
            writer.dump()

            self.assert_stub(path, expected_stub)

    def assert_stub(self, path: Path, expected: str) -> None:
        self.assertEqual(path.read_text().strip(), dedent(expected).strip())


if __name__ == "__main__":
    unittest.main()
