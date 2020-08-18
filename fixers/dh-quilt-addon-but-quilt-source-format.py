#!/usr/bin/python3


from lintian_brush.fixer import report_result
from lintian_brush.rules import (
    dh_invoke_drop_with,
    RulesEditor,
    )
from lintian_brush.patches import rules_find_patches_directory


def drop_quilt_with(line, target):
    return dh_invoke_drop_with(line, b'quilt')


try:
    with open('debian/source/format', 'r') as f:
        if f.read().strip() == '3.0 (quilt)':
            with RulesEditor() as updater:
                if rules_find_patches_directory(
                        updater.makefile) in ('debian/patches', None):
                    updater.legacy_update(command_line_cb=drop_quilt_with)
except FileNotFoundError:
    pass


report_result(
    "Don't specify --with=quilt, since package uses "
    "'3.0 (quilt)' source format.",
    fixed_lintian_tags=['dh-quilt-addon-but-quilt-source-format'])
