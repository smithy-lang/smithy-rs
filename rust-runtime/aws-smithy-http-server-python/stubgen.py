#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import inspect
import re
import textwrap
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple

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
    generics: Set[str]

    def __init__(self, path: Path, root_module_name: str) -> None:
        self.path = path
        self.root_module_name = root_module_name
        self.subwriters = []
        self.imports = set([])
        self.defs = []
        self.generics = set([])

    def fix_path(self, path: str) -> str:
        """
        Returns fixed version of given type path.
        It unescapes `\\[` and `\\]` and also populates placeholder for root module name.
        """
        return path.replace(ROOT_MODULE_NAME_PLACEHOLDER, self.root_module_name).replace("\\[", "[").replace("\\]", "]")

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

    def generic(self, name: str) -> None:
        self.generics.add(name)

    def dump(self) -> None:
        for w in self.subwriters:
            w.dump()

        generics = ""
        for g in sorted(self.generics):
            generics += f"{g} = {self.include('typing.TypeVar')}('{g}')\n"

        self.path.parent.mkdir(parents=True, exist_ok=True)
        contents = join([f"import {p}" for p in sorted(self.imports)])
        contents += "\n\n"
        if generics:
            contents += generics + "\n"
        contents += join(self.defs)
        self.path.write_text(contents)


class DocstringParserResult:
    def __init__(self) -> None:
        self.types: List[str] = []
        self.params: List[Tuple[str, str]] = []
        self.rtypes: List[str] = []
        self.generics: List[str] = []
        self.extends: List[str] = []


def parse_type_directive(line: str, res: DocstringParserResult):
    parts = line.split(" ", maxsplit=1)
    if len(parts) != 2:
        raise ValueError(f"Invalid `:type` directive: `{line}` must be in `:type T:` format")
    res.types.append(parts[1].rstrip(":"))


def parse_rtype_directive(line: str, res: DocstringParserResult):
    parts = line.split(" ", maxsplit=1)
    if len(parts) != 2:
        raise ValueError(f"Invalid `:rtype` directive: `{line}` must be in `:rtype T:` format")
    res.rtypes.append(parts[1].rstrip(":"))


def parse_param_directive(line: str, res: DocstringParserResult):
    parts = line.split(" ", maxsplit=2)
    if len(parts) != 3:
        raise ValueError(f"Invalid `:param` directive: `{line}` must be in `:param name T:` format")
    name = parts[1]
    ty = parts[2].rstrip(":")
    res.params.append((name, ty))


def parse_generic_directive(line: str, res: DocstringParserResult):
    parts = line.split(" ", maxsplit=1)
    if len(parts) != 2:
        raise ValueError(f"Invalid `:generic` directive: `{line}` must be in `:generic T:` format")
    res.generics.append(parts[1].rstrip(":"))


def parse_extends_directive(line: str, res: DocstringParserResult):
    parts = line.split(" ", maxsplit=1)
    if len(parts) != 2:
        raise ValueError(f"Invalid `:extends` directive: `{line}` must be in `:extends Base[...]:` format")
    res.extends.append(parts[1].rstrip(":"))


DocstringParserDirectives = {
    "type": parse_type_directive,
    "param": parse_param_directive,
    "rtype": parse_rtype_directive,
    "generic": parse_generic_directive,
    "extends": parse_extends_directive,
}


class DocstringParser:
    """
    DocstringParser provides utilities for parsing type information from docstring.
    """

    @staticmethod
    def parse(obj: Any) -> Optional[DocstringParserResult]:
        doc = inspect.getdoc(obj)
        if not doc:
            return None

        res = DocstringParserResult()
        for line in doc.splitlines():
            line = line.strip()
            for d, p in DocstringParserDirectives.items():
                if line.startswith(f":{d} ") and line.endswith(":"):
                    p(line, res)
        return res

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

    @staticmethod
    def parse_class(obj: Any) -> Tuple[List[str], List[str]]:
        result = DocstringParser.parse(obj)
        if not result:
            return ([], [])
        return (result.generics, result.extends)

    @staticmethod
    def clean_doc(obj: Any) -> str:
        doc = inspect.getdoc(obj)
        if not doc:
            return ""

        def predicate(line: str) -> bool:
            for k in DocstringParserDirectives.keys():
                if line.startswith(f":{k} ") and line.endswith(":"):
                    return False
            return True

        return "\n".join([line for line in doc.splitlines() if predicate(line)]).strip()


def indent(code: str, level: int = 4) -> str:
    return textwrap.indent(code, level * " ")


def is_fn_like(obj: Any) -> bool:
    return (
        inspect.isbuiltin(obj)
        or inspect.ismethod(obj)
        or inspect.isfunction(obj)
        or inspect.ismethoddescriptor(obj)
        or inspect.iscoroutine(obj)
        or inspect.iscoroutinefunction(obj)
    )


def is_scalar(obj: Any) -> bool:
    return isinstance(obj, (str, float, int, bool))


def join(args: List[str], delim: str = "\n") -> str:
    return delim.join(filter(lambda x: x, args))


def make_doc(obj: Any) -> str:
    doc = DocstringParser.clean_doc(obj)
    doc = textwrap.dedent(doc)
    if not doc:
        return ""

    return join(['"""', doc, '"""'])


def make_field(writer: Writer, name: str, field: Any) -> str:
    return f"{name}: {writer.fix_and_include(DocstringParser.parse_type(field))}"


def make_function(
    writer: Writer,
    name: str,
    obj: Any,
    include_docs: bool = True,
    parent: Optional[Any] = None,
) -> str:
    is_static_method = False
    if parent and isinstance(obj, staticmethod):
        # Get real method instance from `parent` if `obj` is a `staticmethod`
        is_static_method = True
        obj = getattr(parent, name)

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
    except Exception:
        pass

    def has_default(param: str, ty: str) -> bool:
        # PyO3 allows omitting `Option<T>` params while calling a Rust function from Python,
        # we should always mark `typing.Optional[T]` values as they have default values to allow same
        # flexibiliy as runtime dynamics in type-stubs.
        if ty.startswith("typing.Optional["):
            return True

        if sig is None:
            return False

        sig_param = sig.parameters.get(param)
        return sig_param is not None and sig_param.default is not sig_param.empty

    receivers: List[str] = []
    attrs: List[str] = []
    if parent:
        if is_static_method:
            attrs.append("@staticmethod")
        else:
            receivers.append("self")

    def make_param(name: str, ty: str) -> str:
        fixed_ty = writer.fix_and_include(ty)
        param = f"{name}: {fixed_ty}"
        if has_default(name, fixed_ty):
            param += " = ..."
        return param

    params = join(receivers + [make_param(n, t) for n, t in params], delim=", ")
    attrs_str = join(attrs)
    rtype = writer.fix_and_include(rtype)
    body = "..."
    if include_docs:
        body = join([make_doc(obj), body])

    return f"""
{attrs_str}
def {name}({params}) -> {rtype}:
{indent(body)}
""".lstrip()


def make_class(writer: Writer, name: str, klass: Any) -> str:
    bases = list(filter(lambda n: n != "object", map(lambda b: b.__name__, klass.__bases__)))
    class_sig = DocstringParser.parse_class(klass)
    if class_sig:
        (generics, extends) = class_sig
        bases.extend(map(writer.fix_and_include, extends))
        for g in generics:
            writer.generic(g)

    members: List[str] = []

    class_vars: Dict[str, Any] = vars(klass)
    for member_name, member in sorted(class_vars.items(), key=lambda k: k[0]):
        if member_name.startswith("__"):
            continue

        if inspect.isdatadescriptor(member):
            members.append(
                join(
                    [
                        make_field(writer, member_name, member),
                        make_doc(member),
                    ]
                )
            )
        elif is_fn_like(member):
            members.append(
                make_function(writer, member_name, member, parent=klass),
            )
        elif isinstance(member, klass):
            # Enum variant
            members.append(
                join(
                    [
                        f"{member_name}: {name}",
                        make_doc(member),
                    ]
                )
            )
        else:
            print(f"Unknown member type: {member}")

    if inspect.getdoc(klass) is not None:
        constructor_sig = DocstringParser.parse(klass)
        if constructor_sig is not None and (
            # Make sure to only generate `__init__` if the class has a constructor defined
            len(constructor_sig.rtypes) > 0
            or len(constructor_sig.params) > 0
        ):
            members.append(
                make_function(
                    writer,
                    "__init__",
                    klass,
                    include_docs=False,
                    parent=klass,
                )
            )

    bases_str = "" if len(bases) == 0 else f"({join(bases, delim=', ')})"
    doc = make_doc(klass)
    if doc:
        doc += "\n"
    body = join([doc, join(members, delim="\n\n") or "..."])
    return f"""\
class {name}{bases_str}:
{indent(body)}
"""


def walk_module(writer: Writer, mod: Any):
    exported = mod.__all__

    for name, member in inspect.getmembers(mod):
        if name not in exported:
            continue

        if inspect.ismodule(member):
            subpath = writer.path.parent / name / "__init__.pyi"
            walk_module(writer.submodule(subpath), member)
        elif inspect.isclass(member):
            writer.define(make_class(writer, name, member))
        elif is_fn_like(member):
            writer.define(make_function(writer, name, member))
        elif is_scalar(member):
            writer.define(f"{name}: {type(member).__name__} = ...")
        else:
            print(f"Unknown type: {member}")


def generate(module: str, outdir: str):
    path = Path(outdir) / "__init__.pyi"
    writer = Writer(
        path,
        module,
    )
    walk_module(
        writer,
        importlib.import_module(module),
    )
    writer.dump()


if __name__ == "__main__":
    import argparse
    import importlib

    parser = argparse.ArgumentParser()
    parser.add_argument("module")
    parser.add_argument("outdir")
    args = parser.parse_args()

    generate(args.module, args.outdir)
