from io import BytesIO
from debian.deb822 import Deb822
import sys

def update_control(path='debian/control', **kwargs):
    outf = BytesIO()
    with open(path, 'rb') as f:
        contents = f.read()
    if "DO NOT EDIT" in contents:
        raise Exception("control file not editable")
    update_control_file(BytesIO(contents), outf, **kwargs)
    with open(path, 'wb') as f:
        f.write(outf.getvalue())


def update_control_file(inf, outf, source_package_cb=None, binary_package_cb=None):
    first = True
    for paragraph in Deb822.iter_paragraphs(inf, encoding='utf-8'):
        if paragraph.get("Source"):
            source_package_cb(paragraph)
        else:
            binary_package_cb(paragraph)
        if paragraph:
            if not first:
                outf.write('\n')
            paragraph.dump(fd=outf, encoding='utf-8')
            first = False
