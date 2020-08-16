#!/usr/bin/python3
from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    for key in list(updater.source):
        if key.startswith('XS-Vcs-'):
            updater.source[key[3:]] = updater.source[key]
            del updater.source[key]
            fixed_lintian_tag(
                updater.source, 'xs-vcs-field-in-debian-control',
                info=key)


report_result(
    "Remove unnecessary XS- prefix for Vcs- fields in debian/control.")
