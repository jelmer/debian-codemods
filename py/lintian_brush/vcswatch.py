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

"""Support for accessing the VCS Watch database."""


class VcsWatchError(Exception):
    """Error from vcswatch."""


class VcsWatch:
    """Read VcsWatch data through UDD."""

    def __init__(self):
        self._conn = None

    def __aenter__(self):
        from .udd import connect_udd_mirror

        self._conn = connect_udd_mirror()

    def __aexit__(self, exc_type, exc, tb):
        if self._conn is None:
            raise RuntimeError("not in context manager")
        return self._conn.__exit__(exc_type, exc, tb)

    def get_package(self, name):
        """Get the VCS information for a package.

        Args:
          name: Package name
        Returns:
          Tuple with (vcs_type, vcs_url, vcs_browser)
        """
        assert self._conn is not None, "call connect() first"
        with self._conn.cursor() as cursor:
            cursor.execute(
                """
select vcs, url, browser, status, error from vcswatch
where source = %s""",
                (name,),
            )
            row = cursor.fetchone()
            if row is None:
                raise KeyError(name)
            if row[3] == "ERROR":
                raise VcsWatchError(row[4])
            return row[:3]

    def get_branch_from_url(self, vcs, url):
        """Get the branch for a VCS URL."""
        assert self._conn is not None, "call connect() first"
        with self._conn.cursor() as cursor:
            cursor.execute(
                "select branch, status, error from vcswatch "
                "where url = %s and vcs = %s",
                (url, vcs),
            )
            row = cursor.fetchone()
            if row is None:
                raise KeyError(url)
            if row[1] == "ERROR":
                raise VcsWatchError(row[2])
            return row[0]
