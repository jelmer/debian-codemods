#!/usr/bin/python3

import sys

from debian.changelog import get_maintainer
from lintian_brush.fixer import (
    control,
    fixed_lintian_tag,
    meets_minimum_certainty,
    report_result,
)

# TODO(jelmer): Bump this up if there's a way that we can verify that e.g. the
# ITP was filed by get_maintainer() ?
CERTAINTY = 'possible'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)


with control as updater:
    if updater.source.get('Maintainer'):
        sys.exit(0)
    maintainer = get_maintainer()
    updater.source['Maintainer'] = "{} <{}>".format(*maintainer)
    fixed_lintian_tag(updater.source, 'required-field', 'Maintainer')

report_result(
    'Set the maintainer field to: {} <{}>.'.format(*maintainer),
    certainty=CERTAINTY)
