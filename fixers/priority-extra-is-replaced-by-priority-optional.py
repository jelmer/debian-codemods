#!/usr/bin/python3

from lintian_brush.fixer import control, fixed_lintian_tag, report_result

with control as updater:
    for para in updater.paragraphs:
        if para.get("Priority") == "extra":
            para["Priority"] = "optional"
            fixed_lintian_tag(
                para, "priority-extra-is-replaced-by-priority-optional"
            )


report_result("Change priority extra to priority optional.")
