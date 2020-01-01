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

"""Tests for lintian_brush.systemd."""

from breezy.tests import (
    TestCaseWithTransport,
    )

from io import StringIO

from lintian_brush.systemd import (
    MissingSectionHeaderError,
    UnitFile,
    systemd_service_files,
    update_service_file,
    update_service,
    )


class UnitFileParser(TestCaseWithTransport):

    def test_multiple(self):
        f = StringIO("""\
[Service]
Before=a.service
Before=b.service
""")
        uf = UnitFile(f)
        self.assertEqual(['Service'], list(uf))
        self.assertEqual(
            ['a.service', 'b.service'],
            list(uf['Service']['Before']))

    def test_multiple_default(self):
        f = StringIO("""\
[Service]
""")
        uf = UnitFile(f)
        self.assertEqual([], list(uf['Service']['Before']))

    def test_multiple_add(self):
        f = StringIO("""\
[Service]
Before=a.service
Before=b.service
""")
        uf = UnitFile(f)
        uf['Service']['Before'] = ['a.service', 'b.service', 'c.service']
        self.assertEqual(['Service'], list(uf))
        self.assertEqual(
            ['a.service', 'b.service', 'c.service'],
            list(uf['Service']['Before']))
        self.assertEqual(str(uf), """\
[Service]
Before=a.service
Before=b.service
Before=c.service
""")

    def test_multiple_set(self):
        f = StringIO("""\
[Service]
Before=a.service
Before=b.service
""")
        uf = UnitFile(f)
        uf['Service']['Before'][0] = 'a.service'
        uf['Service']['Before'][1] = 'c.service'
        self.assertRaises(IndexError, uf['Service']['Before'].__getitem__, 3)
        self.assertEqual(
            ['a.service', 'c.service'],
            list(uf['Service']['Before']))
        self.assertEqual(str(uf), """\
[Service]
Before=a.service
Before=c.service
""")

    def test_multiple_remove(self):
        f = StringIO("""\
[Service]
Before=a.service
Before=b.service
""")
        uf = UnitFile(f)
        uf['Service']['Before'] = ['a.service']
        self.assertEqual(['Service'], list(uf))
        self.assertEqual(str(uf), """\
[Service]
Before=a.service
""")
        self.assertEqual(['a.service'], list(uf['Service']['Before']))

    def test_multiple_reset(self):
        f = StringIO("""\
[Service]
Before=a.service
Before=
Before=b.service
""")
        uf = UnitFile(f)
        self.assertEqual(['b.service'], list(uf['Service']['Before']))
        self.assertEqual(str(uf), """\
[Service]
Before=a.service
Before=
Before=b.service
""")

    def test_setting_before_section(self):
        f = StringIO("""\
Before=a.service

[Service]
Before=b.service
""")
        self.assertRaises(MissingSectionHeaderError, UnitFile, f)


class UpdateServiceFilesTests(TestCaseWithTransport):

    def setUp(self):
        super(UpdateServiceFilesTests, self).setUp()
        self.build_tree_contents([
            ('debian/', ),
            ('debian/other', ''),
            ('debian/apcupsd.service', """\
[Unit]
# network-online is really needed, otherwise there are problems with snmp
# -> 865620
After=network-online.target
Description=UPS power management daemon
Documentation=man:apcupsd(8)

""")])

    def test_systemd_service_files(self):
        self.assertEqual(
            ['debian/apcupsd.service'],
            list(systemd_service_files('debian')))

    def test_update_service_file(self):
        def cb(section, name, value):
            if section == "Unit" and name == "Description":
                return "Not really a UPS pmd"
            return value
        update_service_file('debian/apcupsd.service', cb)
        self.assertFileEqual("""\
[Unit]
# network-online is really needed, otherwise there are problems with snmp
# -> 865620
After=network-online.target
Description=Not really a UPS pmd
Documentation=man:apcupsd(8)

""", 'debian/apcupsd.service')

    def test_update_service(self):
        def cb(section, name, value):
            if section == "Unit" and name == "Description":
                return "Not really a UPS pmd"
            return value
        update_service(cb)
        self.assertFileEqual("""\
[Unit]
# network-online is really needed, otherwise there are problems with snmp
# -> 865620
After=network-online.target
Description=Not really a UPS pmd
Documentation=man:apcupsd(8)

""", 'debian/apcupsd.service')
