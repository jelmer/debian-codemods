#!/usr/bin/python3

import re
from ruamel.yaml import YAML

yaml = YAML()
with open('fixers/index.desc') as f:
    fixers = yaml.load(f)

supported_tags = set()
for fixer in fixers:
    try:
        tags = fixer['lintian-tags']
    except KeyError:
        pass
    else:
        if tags is not None:
            supported_tags.update(tags)

with open("README.md") as f:
    contents = f.read()

replacement_text = "".join([f"* {tag}\n" for tag in sorted(supported_tags)])

with open("README.md", "w") as f:
    # TODO(jelmer): Use better sentinels, just in case somebody changes
    # the current ones?
    f.write(
        re.sub(
            r"(subset of the issues:\n\n).*(\n\.\. _writing-fixers:\n)",
            "\\1" + replacement_text + "\\2",
            contents,
            flags=re.MULTILINE | re.DOTALL,
        )
    )
