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

from lintian_brush.systemd import (
    systemd_service_files,
    update_service_file,
    update_service,
    )


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
            if section == b"Unit" and name == b"Description":
                return b"Not really a UPS pmd"
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
            if section == b"Unit" and name == b"Description":
                return b"Not really a UPS pmd"
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
