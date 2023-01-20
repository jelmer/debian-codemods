#!/usr/bin/python3

from contextlib import suppress
from debmutate.copyright import CopyrightEditor, NotMachineReadableError
from lintian_brush.fixer import report_result, fixed_lintian_tag

renames = [
    ("Name", "Upstream-Name", "upstream_name", False),
    ("Contact", "Upstream-Contact", "upstream_contact", True),
    ("Maintainer", "Upstream-Contact", "upstream_contact", True),
    ("Upstream-Maintainer", "Upstream-Contact", "upstream_contact", True),
    ("Format-Specification", "Format", "format", False),
    ]

applied_renames = []


def obsolete_field(copyright):
    for (old_name, new_name, field_name, multi_line) in renames:
        try:
            value = copyright.header[old_name]
        except KeyError:
            pass
        else:
            if value:
                if multi_line:
                    setattr(
                        copyright.header, field_name,
                        getattr(copyright.header, field_name) + (value,))
                else:
                    setattr(copyright.header, field_name, value)
                applied_renames.append((old_name, new_name))
            del copyright.header[old_name]
            fixed_lintian_tag(
                'source', 'obsolete-field-in-dep5-copyright',
                info=f'{old_name} {new_name}')


with suppress(FileNotFoundError, NotMachineReadableError), \
        CopyrightEditor() as updater:
    obsolete_field(updater.copyright)
report_result(
    "Update copyright file header to use current field names (%s)" %
    ', '.join(f"{key} ⇒ {value}" for (key, value) in applied_renames))
