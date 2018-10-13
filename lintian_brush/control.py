#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

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
