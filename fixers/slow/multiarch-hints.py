#!/usr/bin/python3

from lintian_brush import (
    min_certainty,
    )
from lintian_brush.multiarch_hints import (
    apply_multiarch_hints,
    download_multiarch_hints,
    multiarch_hints_by_binary,
    parse_multiarch_hints,
    )

import os
import sys

if os.environ.get('NET_ACCESS', 'disallow') != 'allow':
    sys.exit(0)

with download_multiarch_hints() as f:
    hints = multiarch_hints_by_binary(parse_multiarch_hints(f))

changes = apply_multiarch_hints(hints, os.environ.get('MINIMUM_CERTAINTY'))

overall_certainty = min_certainty(
    [certainty for (binary, hint, description, certainty) in changes])

print("Apply multi-arch hints.")
print("")

for (binary, hint, description, certainty) in changes:
    print("* %s: %s" % (binary['Package'], description))
