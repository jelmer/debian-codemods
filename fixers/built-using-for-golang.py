#!/usr/bin/python3

from lintian_brush.control import (
    ControlUpdater,
    add_dependency,
    drop_dependency,
    get_relation,
    iter_relations,
    )

added = []
removed = []
go_package = False
default_architecture = None


with ControlUpdater() as updater:
    if any(iter_relations(updater.source.get('Build-Depends', ''),
                          'golang-go')):
        go_package = True
    if any(iter_relations(updater.source.get('Build-Depends', ''),
                          'golang-any')):
        go_package = True
    default_architecture = updater.source.get('Architecture')

    for binary in updater.binaries:
        if binary.get('Architecture', default_architecture) == 'all':
            if 'Built-Using' in binary:
                binary['Built-Using'] = drop_dependency(
                    binary['Built-Using'], '${misc:Built-Using}')
                if not binary['Built-Using']:
                    del binary['Built-Using']
                removed.append(binary['Package'])
        else:
            if go_package:
                built_using = binary.get('Built-Using', '')
                try:
                    get_relation(built_using, "${misc:Built-Using}")
                except KeyError:
                    binary["Built-Using"] = add_dependency(
                        built_using, "${misc:Built-Using}")
                    added.append(binary['Package'])

if added:
    print('Add missing ${misc:Built-Using} to Built-Using on %s.' %
          ', '.join(added))
if removed:
    print('Remove unnecessary ${misc:Built-Using} for %s' %
          ', '.join(removed))
print('Fixed-Lintian-Tags: '
      'missing-built-using-field-for-golang-package')
