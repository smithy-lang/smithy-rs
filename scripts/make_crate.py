#!/usr/bin/env python
import pathlib
from pathlib import Path
import sys
import json
import os
from string import Template

def main():
    print(sys.argv[1])
    args = json.loads(sys.argv[1])
    crate_name = args['crate_name']
    try:
        os.makedirs(f'build/{crate_name}/src')
    except FileExistsError:
        pass
    templates = []
    for root, dirs, files in os.walk("templates"):
        templates += [Path(root) / f for f in files]

    for template in templates:
        with open(template) as f:
            contents = f.read()
        templated = Template(contents).substitute(args)
        final_path = pathlib.Path(*template.parts[1:])
        with open(f'build/{crate_name}/{final_path}', 'w') as f:
            f.write(templated)


if __name__ == "__main__":
    main()
