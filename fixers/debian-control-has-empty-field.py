#!/usr/bin/python3
from lintian_brush.fixer import control, report_result, LintianIssue
fields = []
packages = []


with control as updater:
    for para in updater.paragraphs:
        for k, v in para.items():
            if not v.strip():
                if para.get("Package"):
                    issue = LintianIssue(
                        updater.source, 'debian-control-has-empty-field',
                        info='field "%s" in package %s' % (
                            k, para['Package']))
                    if not issue.should_fix():
                        continue
                    issue.report_fixed()
                    packages.append(para["Package"])
                else:
                    issue = LintianIssue(
                        updater.source, 'debian-control-has-empty-field',
                        info='field "%s" in source paragraph' % (k, ))
                    if not issue.should_fix():
                        continue
                    issue.report_fixed()
                fields.append(k)
                del para[k]

report_result(
    "debian/control: Remove empty control field%s %s%s." % (
     "s" if len(fields) > 1 else "",
     ", ".join(fields),
     (" in package %s" % ', '.join(packages)) if packages else "",
    ))
