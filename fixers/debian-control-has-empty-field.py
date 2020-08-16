#!/usr/bin/python3
from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag
fields = []
packages = []


with ControlEditor() as updater:
    for para in updater.paragraphs:
        for k, v in para.items():
            if not v.strip():
                fields.append(k)
                if para.get("Package"):
                    packages.append(para["Package"])
                    fixed_lintian_tag(
                        updater.source, 'debian-control-has-empty-field',
                        info='field "%s" in package %s' % (
                            k, para['Package']))
                else:
                    fixed_lintian_tag(
                        updater.source, 'debian-control-has-empty-field',
                        info='field "%s" in source paragraph' % (k, ))
                del para[k]

report_result(
    "debian/control: Remove empty control field%s %s%s." % (
     "s" if len(fields) > 1 else "",
     ", ".join(fields),
     (" in package %s" % ', '.join(packages)) if packages else "",
    ))
