#!/usr/bin/python3

from debian.changelog import get_maintainer
from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    report_result, meets_minimum_certainty, fixed_lintian_tag,
    )
import sys

# TODO(jelmer): Bump this up if there's a way that we can verify that e.g. the
# ITP was filed by get_maintainer() ?
CERTAINTY = 'possible'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)


with ControlEditor() as updater:
    if updater.source.get('Maintainer'):
        sys.exit(0)
    maintainer = get_maintainer()
    updater.source['Maintainer'] = "%s <%s>" % maintainer
    fixed_lintian_tag(updater.source, 'required-field', 'Maintainer')

report_result(
    'Set the maintainer field to: %s <%s>.' % maintainer,
    certainty=CERTAINTY)
