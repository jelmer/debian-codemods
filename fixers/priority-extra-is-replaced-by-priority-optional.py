#!/usr/bin/python3

from debmutate.control import ControlEditor


with ControlEditor() as updater:
    for para in updater.paragraphs:
        if para.get("Priority") == "extra":
            para["Priority"] = "optional"


print("Change priority extra to priority optional.")
print("Fixed-Lintian-Tags: priority-extra-is-replaced-by-priority-optional")
