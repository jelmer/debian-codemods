#!/usr/bin/python3

import os
import sys

try:
    import asyncpg  # noqa: F401
except ModuleNotFoundError:
    sys.exit(2)

from lintian_brush.lintian_overrides import remove_unused
from lintian_brush.fixer import net_access_allowed, report_result

if int(os.environ.get('DILIGENCE', '0')) < 1:
    # Removing unused overrides requires pro-actively contacting UDD.
    sys.exit(0)

if not net_access_allowed():
    sys.exit(0)

removed = remove_unused()

description = [
    'Remove %d unused lintian overrides.\n' % len(removed),
    '\n',
    ]
for override in removed:
    description.append('* %s\n' % override.tag)

report_result(
    ''.join(description),
    fixed_lintian_tags=['unused-override'],
    certainty='certain')
