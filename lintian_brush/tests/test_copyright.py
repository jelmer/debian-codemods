#!/usr/bin/python
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

"""Tests for lintian_brush.copyright."""

from debian.copyright import (
    FilesParagraph,
    License,
    )

from breezy.tests import (
    TestCaseWithTransport,
    )

from lintian_brush.copyright import (
    NotMachineReadableError,
    update_copyright,
    )
from lintian_brush.reformatting import (
    FormattingUnpreservable,
    )


class UpdateControlTests(TestCaseWithTransport):

    def test_unpreservable(self):
        self.build_tree_contents([('debian/', ), ('debian/copyright', """\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: lintian-brush
Upstream-Contact: Jelmer <jelmer@samba.org>


Files: *
License: GPL
Copyright: 2012...
""")])

        def dummy(cb):
            cb.header.upstream_name = 'llintian-brush'
        self.assertRaises(FormattingUnpreservable, update_copyright, dummy)

    def test_old_style(self):
        self.build_tree_contents([('debian/', ), ('debian/copyright', """\
This package was debianized in 1995 by Joe Example <joe@example.com>

It was downloaded from ftp://ftp.example.com/pub/blah.
""")])
        self.assertRaises(NotMachineReadableError, update_copyright, None)

    def test_modify(self):
        self.build_tree_contents([('debian/', ), ('debian/copyright', """\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: lintian-brush
Upstream-Contact: Jelmer <jelmer@samba.org>

Files: *
License: GPL
Copyright: 2012...
""")])

        def add_stanza(copyright):
            copyright.add_files_paragraph(FilesParagraph.create(
                ['foo.c'], "2012 Joe Example",
                License("Apache")))
        self.assertTrue(update_copyright(add_stanza))
        self.assertFileEqual("""\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: lintian-brush
Upstream-Contact: Jelmer <jelmer@samba.org>

Files: *
License: GPL
Copyright: 2012...

Files: foo.c
Copyright: 2012 Joe Example
License: Apache
""", 'debian/copyright')
