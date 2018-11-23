#!/usr/bin/python3

from lintian_brush.control import update_control

packages = []
default_priority = None


def get_source_section(control):
    global default_priority
    default_priority = control["Priority"]


def oldlibs_priority_optional(control):
    # TODO(jelmer): needs higher certainty?
    if "transitional package" in control["Description"]:
        packages.append(control["Package"])
        control["Section"] = "oldlibs"
        if default_priority != "optional":
            control["Priority"] = "optional"
        elif "Priority" in control:
            del control["Priority"]


update_control(binary_package_cb=oldlibs_priority_optional,
               source_package_cb=get_source_section)
print("Move transitional package%s %s to oldlibs/optional per policy 4.0.1." %
      (("s" if len(packages) > 1 else ""), ", ".join(packages)))
print("Fixed-Lintian-Tags: transitional-package-should-be-oldlibs-optional")
