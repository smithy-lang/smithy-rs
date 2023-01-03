#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

from __future__ import annotations
import re
import inspect
import textwrap
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Set, List, Tuple, Optional

ROOT_MODULE_NAME_PLACEHOLDER = "__root_module_name__"


class Writer:
    """
    Writer provides utilities for writing Python stubs.
    """

    root_module_name: str
    path: Path
    subwriters: List[Writer]
    imports: Set[str]
    defs: List[str]

    def __init__(self, path: Path, root_module_name: str) -> None:
        self.path = path
        self.root_module_name = root_module_name
        self.subwriters = []
        self.imports = set([])
        self.defs = []

    def fix_path(self, path: str) -> str:
        return path.replace(ROOT_MODULE_NAME_PLACEHOLDER, self.root_module_name)

    def submodule(self, path: Path) -> Writer:
        w = Writer(path, self.root_module_name)
        self.subwriters.append(w)
        return w

    def include(self, path: str) -> str:
        # `path` might be nested like: typing.Optional[typing.List[pokemon_service_server_sdk.model.GetPokemonSpecies]]
        # we need to process every subpath in a nested path
        paths = filter(lambda p: p, re.split("\\[|\\]|,| ", path))
        for subpath in paths:
            parts = subpath.rsplit(".", maxsplit=1)
            # add `typing` to imports for a path like `typing.List`
            # but skip if the path doesn't have any namespace like `str` or `bool`
            if len(parts) == 2:
                self.imports.add(parts[0])

        return path

    def fix_and_include(self, path: str) -> str:
        return self.include(self.fix_path(path))

    def define(self, code: str) -> None:
        self.defs.append(code)

    def dump(self) -> None:
        for w in self.subwriters:
            w.dump()

        self.path.parent.mkdir(parents=True, exist_ok=True)
        contents = "\n".join(map(lambda p: f"import {p}", self.imports))
        contents += "\n\n"
        contents += "\n".join(self.defs)
        self.path.write_text(contents)


@dataclass
class DocstringParserResult:
    types: List[str]
    params: List[Tuple[str, str]]
    rtypes: List[str]


class DocstringParser:
    """
    DocstringParser provides utilities for parsing type information from docstring.
    """

    @staticmethod
    def parse(obj: Any) -> Optional[DocstringParserResult]:
        doc = inspect.getdoc(obj)
        if not doc:
            return None

        types: List[str] = []
        params: List[Tuple[str, str]] = []
        rtypes: List[str] = []

        for line in doc.splitlines():
            line = line.strip()
            if line.startswith(":type ") and line.endswith(":"):
                parts = line.split(" ", maxsplit=1)
                if len(parts) != 2:
                    raise ValueError(
                        f"Invalid `:type` directive: `{line}` must be in `:type T:` format"
                    )
                types.append(parts[1].rstrip(":"))
            elif line.startswith(":param ") and line.endswith(":"):
                parts = line.split(" ", maxsplit=2)
                if len(parts) != 3:
                    raise ValueError(
                        f"Invalid `:param` directive: `{line}` must be in `:param name T:` format"
                    )
                name = parts[1]
                ty = parts[2].rstrip(":")
                params.append((name, ty))
            elif line.startswith(":rtype ") and line.endswith(":"):
                parts = line.split(" ", maxsplit=1)
                if len(parts) != 2:
                    raise ValueError(
                        f"Invalid `:rtype` directive: `{line}` must be in `:rtype T:` format"
                    )
                rtypes.append(parts[1].rstrip(":"))

        return DocstringParserResult(types=types, params=params, rtypes=rtypes)

    @staticmethod
    def parse_type(obj: Any) -> str:
        result = DocstringParser.parse(obj)
        if not result or len(result.types) == 0:
            return "typing.Any"
        return result.types[0]

    @staticmethod
    def parse_function(obj: Any) -> Optional[Tuple[List[Tuple[str, str]], str]]:
        result = DocstringParser.parse(obj)
        if not result:
            return None

        return (
            result.params,
            "None" if len(result.rtypes) == 0 else result.rtypes[0],
        )


def indent(code: str, level: int) -> str:
    return textwrap.indent(code, level * " ")


def format_doc(obj: Any, indent_level: int) -> str:
    doc = inspect.getdoc(obj)
    if not doc:
        return ""

    # Remove type annotations
    doc = "\n".join(
        filter(
            lambda l: not (
                (
                    l.startswith(":type")
                    or l.startswith(":rtype")
                    or l.startswith(":param")
                )
                and l.endswith(":")
            ),
            doc.splitlines(),
        )
    ).strip()

    if not doc:
        return ""

    return indent('"""\n' + doc + '\n"""\n', indent_level)


def is_fn_like(obj: Any) -> bool:
    return (
        inspect.isbuiltin(obj)
        or inspect.ismethod(obj)
        or inspect.isfunction(obj)
        or inspect.ismethoddescriptor(obj)
    )


def make_field(writer: Writer, name: str, field: Any) -> str:
    return f"{name}: {writer.fix_and_include(DocstringParser.parse_type(field))}"


def make_function(
    writer: Writer,
    name: str,
    obj: Any,
    indent_level: int = 0,
    include_docs: bool = True,
) -> str:
    res = DocstringParser.parse_function(obj)
    if not res:
        # Make it `Any` if we can't parse the docstring
        return f"{name}: {writer.include('typing.Any')}"

    params, rtype = res
    # We're using signature for getting default values only, currently type hints are not supported
    # in signatures. We can leverage signatures more if it supports type hints in future.
    sig: Optional[inspect.Signature] = None
    try:
        sig = inspect.signature(obj)
    except:
        pass

    receivers: List[str] = []
    attrs: List[str] = []
    if inspect.ismethoddescriptor(obj) or name == "__init__":
        receivers.append("self")
    else:
        attrs.append("@staticmethod")

    def format_param(name: str, ty: str) -> str:
        param = f"{name}: {writer.fix_and_include(ty)}"

        if sig is not None:
            sig_param = sig.parameters.get(name)
            if sig_param and sig_param.default is not sig_param.empty:
                param += f" = ..."

        return param

    params = ", ".join(receivers + [format_param(n, t) for n, t in params])

    fn_def = ""
    if len(attrs) > 0:
        for attr in attrs:
            fn_def += indent(f"{attr}\n", indent_level)
    fn_def += indent(
        f"def {name}({params}) -> {writer.fix_and_include(rtype)}:\n", indent_level
    )

    if include_docs:
        fn_def += format_doc(obj, indent_level + 4)

    fn_def += indent("...", indent_level + 4)
    return fn_def


def make_class(
    writer: Writer, class_name: str, klass: Any, indent_level: int = 0
) -> str:
    bases = ", ".join(map(lambda b: b.__name__, klass.__bases__))
    definition = f"class {class_name}({bases}):\n"

    def preserve_doc(obj: Any) -> str:
        return format_doc(obj, indent_level + 4) + "\n"

    definition += preserve_doc(klass)

    is_empty = True
    for (name, member) in inspect.getmembers(klass):
        if name.startswith("__"):
            continue

        if inspect.isdatadescriptor(member):
            is_empty = False
            definition += (
                indent(make_field(writer, name, member), indent_level + 4) + "\n"
            )
            definition += preserve_doc(member)
        elif is_fn_like(member):
            is_empty = False
            definition += make_function(writer, name, member, indent_level + 4) + "\n"
        # Enum variant
        elif isinstance(member, klass):
            is_empty = False
            definition += indent(f"{name}: {class_name}\n", indent_level + 4)
            definition += preserve_doc(member)
        else:
            print(f"Unknown member type={member}")

    if inspect.getdoc(klass) is not None:
        constructor_sig = DocstringParser.parse(klass)
        if constructor_sig is not None and (
            # Make sure to only generate `__init__` if the class has a constructor defined
            len(constructor_sig.rtypes) > 0
            or len(constructor_sig.params) > 0
        ):
            is_empty = False
            definition += (
                make_function(
                    writer, "__init__", klass, indent_level + 4, include_docs=False
                )
                + "\n"
            )

    if is_empty:
        definition += indent(f"...\n", indent_level + 4)

    return definition


def walk_module(writer: Writer, mod: Any):
    exported = mod.__all__

    for (name, member) in inspect.getmembers(mod):
        if name not in exported:
            continue

        if inspect.ismodule(member):
            walk_module(
                writer.submodule(writer.path.parent / name / "__init__.pyi"), member
            )
        elif inspect.isclass(member):
            writer.define(make_class(writer, name, member))
        elif is_fn_like(member):
            writer.define(make_function(writer, name, member))
        else:
            print(f"Unknown type: {member}")


if __name__ == "__main__":
    import argparse
    import importlib

    parser = argparse.ArgumentParser()
    parser.add_argument("module")
    parser.add_argument("outdir")
    args = parser.parse_args()

    path = Path(args.outdir) / f"{args.module}.pyi"
    writer = Writer(
        path,
        args.module,
    )
    walk_module(
        writer,
        importlib.import_module(args.module),
    )
    writer.dump()
