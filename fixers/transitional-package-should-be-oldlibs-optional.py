#!/usr/bin/python3

from lintian_brush.fixer import control, report_result, LintianIssue

packages = []


with control as updater:
    default_priority = updater.source.get("Priority")

    # TODO(jelmer): needs higher certainty?
    for binary in updater.binaries:
        if binary.get("Package-Type") == "udeb":
            continue
        if "transitional package" not in binary.get("Description", ""):
            continue
        issue = LintianIssue(
            binary, 'transitional-package-not-oldlibs-optional',
            '%s/%s' % (
                binary.get('Section') or updater.source.get('Section'),
                binary.get('Priority') or updater.source.get('Priority')))
        if issue.should_fix():
            packages.append(binary["Package"])
            binary["Section"] = "oldlibs"
            if default_priority != "optional":
                binary["Priority"] = "optional"
            elif "Priority" in binary:
                del binary["Priority"]
            issue.report_fixed()


report_result(
    "Move transitional package%s %s to oldlibs/optional per policy 4.0.1." %
    (("s" if len(packages) > 1 else ""), ", ".join(packages)))
