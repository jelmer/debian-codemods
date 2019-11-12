#!/usr/bin/python3

from lintian_brush.control import (
    add_dependency,
    parse_relations,
    update_control,
    )


uses_debhelper = False
misc_depends_added = []


def check_debhelper(control):
    global uses_debhelper
    if "Build-Depends" not in control:
        return
    for entry in parse_relations(control["Build-Depends"]):
        (head_whitespace, relation, tail_whitespace) = entry
        if any(r.name == 'debhelper' for r in relation):
            uses_debhelper = True


def add_misc_depends(control):
    global uses_debhelper
    if not uses_debhelper:
        return
    for entry in parse_relations(control.get("Depends", '')):
        (head_whitespace, relation, tail_whitespace) = entry
        if any(r.name == '${misc:Depends}' for r in relation):
            return
    control["Depends"] = add_dependency(
        control.get("Depends", ''), "${misc:Depends}")
    misc_depends_added.append(control["Package"])


update_control(source_package_cb=check_debhelper,
               binary_package_cb=add_misc_depends)
print("Add missing ${misc:Depends} to Depends for %s." %
      ", ".join(misc_depends_added))
print("Fixed-Lintian-Tags: debhelper-but-no-misc-depends")
