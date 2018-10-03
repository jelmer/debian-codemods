from io import BytesIO
from debian.deb822 import Deb822
import sys

def update_control(path='debian/control', **kwargs):
    outf = BytesIO()
    with open(path, 'rb') as f:
        update_control_file(f, outf, **kwargs)
    with open(path, 'wb') as f:
        f.write(outf.getvalue())


def update_control_file(inf, outf, source_package_cb=None, binary_package_cb=None):
    control = Deb822(inf, encoding='utf-8')
    if source_package_cb:
        source_package_cb(control)
    while control:
        control.dump(fd=outf, encoding='utf-8')
        control = Deb822(inf, encoding='utf-8')
        if control:
            if binary_package_cb:
                binary_package_cb(control)
            outf.write('\n')
