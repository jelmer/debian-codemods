#!/usr/bin/python3

from lintian_brush.control import (
    update_control,
    get_relation,
    add_dependency,
    drop_dependency,
    )


def move_debhelper_compat(control):
    try:
        pos, old = get_relation(
            control.get('Build-Depends-Indep', ''), 'debhelper-compat')
    except KeyError:
        return
    control['Build-Depends'] = add_dependency(
        control.get('Build-Depends', ''), old)
    control['Build-Depends-Indep'] = drop_dependency(
        control.get('Build-Depends-Indep', ''), 'debhelper-compat')
    if not control['Build-Depends-Indep'].strip():
        del control['Build-Depends-Indep']


update_control(source_package_cb=move_debhelper_compat)
print('Move debhelper-compat from Build-Depends-Indep to Build-Depends.')
