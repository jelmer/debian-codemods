#!/usr/bin/python3


from lintian_brush.rules import (
    dh_invoke_drop_with,
    update_rules,
    )


def drop_quilt_with(line, target):
    return dh_invoke_drop_with(line, b'quilt')


update_rules(drop_quilt_with)


print("Don't specify --with=quilt, since package uses "
      "'3.0 (quilt)' source format.")
print("Fixed-Lintian-Tags: dh-quilt-addon-but-quilt-source-format")
