#!/usr/bin/python3

from typing import Optional

from lintian_brush.fixer import LintianIssue, control, report_result

packages = []


with control as updater:
    default_priority = updater.source.get("Priority")

    # TODO(jelmer): needs higher certainty?
    for binary in updater.binaries:
        if binary.get("Package-Type") == "udeb":
            continue
        if "transitional package" not in binary.get("Description", ""):
            continue
        oldsection = binary.get("Section") or updater.source.get("Section")
        issue = LintianIssue(
            binary,
            "transitional-package-not-oldlibs-optional",
            "{}/{}".format(
                oldsection,
                binary.get("Priority") or updater.source.get("Priority"),
            ),
        )
        if issue.should_fix():
            packages.append(binary["Package"])
            area: Optional[str]
            if oldsection and "/" in oldsection:
                area, oldsection = oldsection.split("/", 1)
            else:
                area = None
            if area:
                binary["Section"] = f"{area}/oldlibs"
            else:
                binary["Section"] = "oldlibs"
            if default_priority != "optional":
                binary["Priority"] = "optional"
            elif "Priority" in binary:
                del binary["Priority"]
            issue.report_fixed()


report_result(
    "Move transitional package{} {} to oldlibs/optional per policy 4.0.1.".format(
        ("s" if len(packages) > 1 else ""), ", ".join(packages)
    )
)
