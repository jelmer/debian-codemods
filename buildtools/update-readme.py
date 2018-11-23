#!/usr/bin/python3

from lintian_brush import available_lintian_fixers
import re

with open('README.md', 'r') as f:
    contents = f.read()

fixers = available_lintian_fixers()

tags = set()
for fixer in fixers:
    tags.update(fixer.lintian_tags)
replacement_text = ''.join(['* %s\n' % tag for tag in sorted(tags)])

with open('README.md', 'w') as f:
    # TODO(jelmer): Use better sentinels, just in case somebody changes
    # the current ones?
    f.write(re.sub(
        r'(subset of the issues:\n\n).*(\n\.\. _writing-fixers:\n)',
        '\\1' + replacement_text + '\\2', contents,
        flags=re.MULTILINE | re.DOTALL))
