#!/usr/bin/python
# Copyright (C) 2019 Jelmer Vernooij
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

"""Tests for lintian_brush.multiarch_hints."""

from io import BytesIO

from breezy.tests import (
    TestCase,
    )

from lintian_brush.multiarch_hints import (
    parse_multiarch_hints,
    )


class ParseMultiArchHints(TestCase):

    def test_invalid_header(self):
        f = BytesIO(b"""\
format: blah
""")
        self.assertRaises(ValueError, parse_multiarch_hints, f)

    def test_some_entries(self):
        f = BytesIO(b"""\
format: multiarch-hints-1.0
hints:
- binary: coinor-libcoinmp-dev
  description: coinor-libcoinmp-dev conflicts on ...
  link: https://wiki.debian.org/MultiArch/Hints#file-conflict
  severity: high
  source: coinmp
  version: 1.8.3-2+b11
""")
        self.assertEqual(
            parse_multiarch_hints(f), [{
                'binary': 'coinor-libcoinmp-dev',
                'description': 'coinor-libcoinmp-dev conflicts on ...',
                'link': (
                    'https://wiki.debian.org/MultiArch/Hints#file-conflict'),
                'severity': 'high',
                'source': 'coinmp',
                'version': '1.8.3-2+b11'
              }],
            )
