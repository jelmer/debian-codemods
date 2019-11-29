#!/usr/bin/python3

import os

from lintian_brush import certainty_sufficient
from lintian_brush.control import update_control
from lintian_brush.upstream_metadata import guess_upstream_metadata_items

current_certainty = None


def fill_in_homepage(control):
    global current_certainty
    if 'Homepage' in control:
        return
    minimum_certainty = os.environ.get('MINIMUM_CERTAINTY')
    for datum in guess_upstream_metadata_items(
            '.', trust_package=(os.environ.get('TRUST_PACKAGE') == 'true')):
        if datum.field != 'Homepage':
            continue
        if not certainty_sufficient(datum.certainty, minimum_certainty):
            continue
        if current_certainty != 'certain':
            control["Homepage"] = datum.value
            current_certainty = datum.certainty


update_control(source_package_cb=fill_in_homepage)

print('Fill in Homepage field.')
print('Fixed-Lintian-Tags: no-homepage-field')
if current_certainty:
    print('Certainty: %s' % current_certainty)
