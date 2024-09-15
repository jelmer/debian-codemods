#!/usr/bin/python3

import re
from lintian_brush import fixable_lintian_tags
supported_tags = fixable_lintian_tags()

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
