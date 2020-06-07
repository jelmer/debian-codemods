#!/usr/bin/python3
from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result
fields = []
packages = []


with ControlEditor() as updater:
    for para in updater.paragraphs:
        for k, v in para.items():
            if not v.strip():
                fields.append(k)
                if para.get("Package"):
                    packages.append(para.get("Package"))
                del para[k]

report_result(
    "debian/control: Remove empty control field%s %s%s." % (
     "s" if len(fields) > 1 else "",
     ", ".join(fields),
     (" in package %s" % ', '.join(packages)) if packages else "",
    ),
    fixed_lintian_tags=['debian-control-has-empty-field'])
