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

"""Automatically fix lintian issues."""

from typing import Optional, Sequence

__version__ = (0, 163)
version_string = ".".join(map(str, __version__))
SUPPORTED_CERTAINTIES = ["certain", "confident", "likely", "possible", None]


def min_certainty(certainties: Sequence[str]) -> str:
    """Get the minimum certainty from a list of certainties."""
    return (
        SUPPORTED_CERTAINTIES[
            max([SUPPORTED_CERTAINTIES.index(c) for c in certainties] + [0])
        ]
        or "unknown"
    )


def certainty_to_confidence(certainty: Optional[str]) -> Optional[int]:
    """Convert a certainty string to a confidence level."""
    if certainty in ("unknown", None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


def certainty_sufficient(
    actual_certainty: str, minimum_certainty: Optional[str]
) -> bool:
    """Check if the actual certainty is sufficient.

    Args:
      actual_certainty: Actual certainty with which changes were made
      minimum_certainty: Minimum certainty to keep changes
    Returns:
      boolean
    """
    actual_confidence = certainty_to_confidence(actual_certainty)
    if actual_confidence is None:
        # Actual confidence is unknown.
        # TODO(jelmer): Should we really be ignoring this?
        return True
    minimum_confidence = certainty_to_confidence(minimum_certainty)
    if minimum_confidence is None:
        return True
    return actual_confidence <= minimum_confidence


def fixable_lintian_tags():
    """Get the set of lintian tags that have fixers."""
    from . import _lintian_brush_rs

    return set(_lintian_brush_rs.get_builtin_fixer_lintian_tags())
