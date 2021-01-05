#!/usr/bin/python3

from lintian_brush.fixer import control, report_result, fixed_lintian_tag

packages = []


with control as updater:
    default_priority = updater.source.get("Priority")

    # TODO(jelmer): needs higher certainty?
    for binary in updater.binaries:
        if "transitional package" in binary.get("Description", ""):
            fixed_lintian_tag(
                binary, 'transitional-package-not-oldlibs-optional',
                '%s/%s' % (
                    binary.get('Section') or updater.source.get('Section'),
                    binary.get('Priority') or updater.source.get('Priority')))
            packages.append(binary["Package"])
            binary["Section"] = "oldlibs"
            if default_priority != "optional":
                binary["Priority"] = "optional"
            elif "Priority" in binary:
                del binary["Priority"]


report_result(
    "Move transitional package%s %s to oldlibs/optional per policy 4.0.1." %
    (("s" if len(packages) > 1 else ""), ", ".join(packages)))
