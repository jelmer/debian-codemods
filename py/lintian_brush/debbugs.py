#!/usr/bin/python3
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

"""Support for accessing the DebBugs database."""

import logging


class DebBugs:
    """Read DebBugs data through UDD."""

    def __init__(self):
        self._conn = None

    def connect(self):
        from .udd import connect_udd_mirror

        self._conn = connect_udd_mirror()

    def check_bug(self, package, bugid):
        """Check that a bug belongs to a particular package.

        Args:
          package: Package name
          bugid: Bug number
        Returns:
          Boolean
        """
        assert self._conn is not None, "call connect() first"
        with self._conn.cursor() as cursor:
            cursor.execute("select package from bugs where id = %s", (bugid,))
            row = cursor.fetchone()
            if row is None:
                return False
            return row[0] == package


def find_archived_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        logging.warning("psycopg2 not available, unable to find wnpp bugs.")
        return []
    conn = connect_udd_mirror()
    with conn.cursor() as cursor:
        cursor.execute(
            """
select id, substring(title, 0, 3) from archived_bugs where package = 'wnpp' and
title like 'ITP: ' || %s || ' -- %%' OR
title like 'RFP: ' || %s || ' -- %%'
""",
            (source_name, source_name),
        )
        return [(row[0], row[1]) for row in cursor]


def find_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        logging.warning("psycopg2 not available, unable to find wnpp bugs.")
        return []
    conn = connect_udd_mirror()
    with conn.cursor() as cursor:
        cursor.execute(
            """
select id, type from wnpp where source = %s and type in ('ITP', 'RFP')
""",
            (source_name,),
        )
        return [(row[0], row[1]) for row in cursor]
