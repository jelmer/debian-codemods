#!/usr/bin/python3

from lintian_brush.control import (
    update_control,
    add_dependency,
    drop_dependency,
    get_relation,
    iter_relations,
    )

added = []
removed = []
go_package = False
default_architecture = None


def check_go_package(control):
    global go_package, default_architecture
    if any(iter_relations(control.get('Build-Depends', ''), 'golang-go')):
        go_package = True
    if any(iter_relations(control.get('Build-Depends', ''), 'golang-any')):
        go_package = True
    default_architecture = control.get('Architecture')


def add_built_using(control):
    if control.get('Architecture', default_architecture) == 'all':
        if 'Built-Using' in control:
            control['Built-Using'] = drop_dependency(
                control['Built-Using'], '${misc:Built-Using}')
            if not control['Built-Using']:
                del control['Built-Using']
            removed.append(control['Package'])
    else:
        if go_package:
            built_using = control.get('Built-Using', '')
            try:
                get_relation(built_using, "${misc:Built-Using}")
            except KeyError:
                control["Built-Using"] = add_dependency(
                    built_using, "${misc:Built-Using}")
                added.append(control['Package'])


update_control(
    binary_package_cb=add_built_using, source_package_cb=check_go_package)

if added:
    print('Add missing ${misc:Built-Using} to Built-Using on %s.' %
          ', '.join(added))
if removed:
    print('Remove unnecessary ${misc:Built-Using} for %s' %
          ', '.join(removed))
print('Fixed-Lintian-Tags: '
      'missing-built-using-field-for-golang-package')
