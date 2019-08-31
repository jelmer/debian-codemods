#!/usr/bin/python3

from lintian_brush.control import (
    update_control,
    add_dependency,
    get_relation,
    )

added = []
removed = []
go_package = False
default_architecture = None


def check_go_package(control):
    global go_package, default_architecture
    try:
        get_relation(control.get('Build-Depends', ''), 'golang-go')
    except KeyError:
        pass
    else:
        go_package = True
    try:
        get_relation(control.get('Build-Depends', ''), 'golang-any')
    except KeyError:
        pass
    else:
        go_package = True
    default_architecture = control['Architecture']


def add_built_using(control):
    if control.get('Architecture', default_architecture) == 'all':
        del control['Built-Using']
        removed.append(control['Package'])
    else:
        control["Built-Using"] = add_dependency(
            control.get('Built-Using', ''), "${misc:Built-Using}")
        added.append(control['Package'])


update_control(binary_package_cb=add_built_using)

if added:
    print('Add missing ${misc:Built-Using} to Built-Using on %s.' %
          ', '.join(added))
if removed:
    print('Remove unnecessary Built-Using on %s' %
          ', '.join(removed))
print('Fixed-Lintian-Tags: '
      'missing-built-using-field-for-golang-package')
