#!/usr/bin/python3

from lintian_brush.control import update_control

default_priority = None
changed = []


def read_source_package(control):
    global default_priority
    default_priority = control.get('Priority')


def drop_library_priority(control):
    if control.get("Section") != 'libs':
        return
    priority = control.get('Priority', default_priority)
    if priority in ("required", "important", "standard"):
        control['Priority'] = 'optional'
        changed.append(control['Package'])


update_control(
    source_package_cb=read_source_package,
    binary_package_cb=drop_library_priority)

print('Set priority for library package%s %s to optional.' % (
      's' if len(changed) > 1 else '', ', '.join(changed)))
print('Fixed-Lintian-Tags: excessive-priority-for-library-package')
