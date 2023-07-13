#!/usr/bin/python3

import os
import sys

from lintian_brush import (
    DEFAULT_MINIMUM_CERTAINTY,
    min_certainty,
)
from lintian_brush.multiarch_hints import (
    apply_multiarch_hints,
    download_multiarch_hints,
    multiarch_hints_by_binary,
    parse_multiarch_hints,
)

if os.environ.get('NET_ACCESS', 'disallow') != 'allow':
    sys.exit(0)

with download_multiarch_hints() as f:
    hints = multiarch_hints_by_binary(parse_multiarch_hints(f))

certainty = os.environ.get('MINIMUM_CERTAINTY', DEFAULT_MINIMUM_CERTAINTY)

changes = apply_multiarch_hints(hints, certainty)

overall_certainty = min_certainty(
    [certainty for (binary, hint, description, certainty) in changes])

print("Apply multi-arch hints.")
print("")

for (binary, _hint, description, _certainty) in changes:
    print("* {}: {}".format(binary['Package'], description))
