#!/usr/bin/python3
# Copyright (C) 2024 Jelmer Vernooij
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

"""Utility functions for dealing with patches."""

__all__ = [
    'find_patches_directory',
    'tree_non_patches_changes',
    'read_quilt_series',
]

from ._lintian_brush_rs import (
    find_patches_directory,
    read_quilt_series,
    tree_non_patches_changes,
)
