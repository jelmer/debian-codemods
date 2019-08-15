#!/usr/bin/python3

from lintian_brush.control import (
    update_control,
    get_relation,
    drop_dependency,
    add_dependency,
    )


def move_libmodule_build_perl(control):
    try:
        get_relation(
            control.get('Build-Depends-Indep', ''), 'libmodule-build-perl')
    except KeyError:
        return

    control['Build-Depends-Indep'] = drop_dependency(
        control['Build-Depends-Indep'], 'libmodule-build-perl')
    control['Build-Depends'] = add_dependency(
        control['Build-Depends'], 'libmodule-build-perl')


update_control(source_package_cb=move_libmodule_build_perl)
print('Move libmodule-build-perl from Build-Depends-Indep to Build-Depends.')
print('Fixed: libmodule-build-perl-needs-to-be-in-build-depend')
