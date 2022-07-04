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


class DebBugs(object):
    """Read DebBugs data through UDD."""

    def __init__(self):
        self._conn = None

    async def connect(self):
        from .udd import connect_udd_mirror

        pool = await connect_udd_mirror()
        self._conn = await pool.__enter__()

    async def check_bug(self, package, bugid):
        """Check that a bug belongs to a particular package.

        Args:
          package: Package name
          bugid: Bug number
        Returns:
          Boolean
        """
        assert self._conn is not None, "call connect() first"
        row = await self._conn.fetchrow(
            """
select package from bugs where id = $1""",
            bugid,
        )
        if row is None:
            return False
        return row[0] == package


async def find_archived_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        logging.warning("asyncpg not available, unable to find wnpp bugs.")
        return []
    async with await connect_udd_mirror() as conn:
        return [
            (row[0], row[1])
            for row in await conn.fetch("""\
select id, substring(title, 0, 3) from archived_bugs where package = 'wnpp' and
title like 'ITP: ' || $1 || ' -- %' OR
title like 'RFP: ' || $1 || ' -- %'
""", source_name)]


async def find_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        logging.warning("asyncpg not available, unable to find wnpp bugs.")
        return []
    async with await connect_udd_mirror() as conn:
        return [
            (row[0], row['type'])
            for row in await conn.fetch("""\
select id, type from wnpp where source = $1 and type in ('ITP', 'RFP')
""", source_name)]
