#!/usr/bin/python3

from debian.changelog import get_maintainer
from lintian_brush.control import ControlUpdater
from lintian_brush.fixer import report_result, meets_minimum_certainty
import sys

# TODO(jelmer): Bump this up if there's a way that we can verify that e.g. the
# ITP was filed by get_maintainer() ?
CERTAINTY = 'possible'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)


with ControlUpdater() as updater:
    if updater.source.get('Maintainer'):
        sys.exit(0)
    maintainer = get_maintainer()
    updater.source['Maintainer'] = "%s <%s>" % maintainer

report_result(
    'Set the maintainer field to: %s <%s>.' % maintainer,
    certainty=CERTAINTY,
    fixed_lintian_tags=['no-maintainer-field'])
