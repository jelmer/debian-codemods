#!/usr/bin/python3

from lintian_brush.copyright import update_copyright, NotMachineReadableError

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


try:
    update_copyright(obsolete_field)
except (FileNotFoundError, NotMachineReadableError):
    pass
print(
    "Update copyright file header to use current field names (%s)" %
    ', '.join("%s => %s" % (key, value) for (key, value) in applied_renames))
print("Fixed-Lintian-Tags: obsolete-field-in-dep5-copyright")
