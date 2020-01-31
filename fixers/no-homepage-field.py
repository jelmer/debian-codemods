#!/usr/bin/python3

import os

from lintian_brush import certainty_sufficient
from lintian_brush.control import ControlUpdater
from lintian_brush.upstream_metadata import guess_upstream_metadata_items

current_certainty = None
trust_package = os.environ.get('TRUST_PACKAGE') == 'true'

with ControlUpdater() as updater:
    if 'Homepage' not in updater.source:
        minimum_certainty = os.environ.get('MINIMUM_CERTAINTY')
        for datum in guess_upstream_metadata_items(
                '.', trust_package=trust_package):
            if datum.field != 'Homepage':
                continue
            if not certainty_sufficient(datum.certainty, minimum_certainty):
                continue
            if current_certainty != 'certain':
                updater.source["Homepage"] = datum.value
                current_certainty = datum.certainty

print('Fill in Homepage field.')
print('Fixed-Lintian-Tags: no-homepage-field')
if current_certainty:
    print('Certainty: %s' % current_certainty)
