#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    for para in updater.paragraphs:
        if para.get("Priority") == "extra":
            para["Priority"] = "optional"
            fixed_lintian_tag(
                para, 'priority-extra-is-replaced-by-priority-optional')


report_result("Change priority extra to priority optional.")
