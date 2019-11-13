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

"""Utility functions for dealing with rules files."""

import os
import re


class Rule(object):
    """A make rule."""

    def __init__(self, firstline):
        self.lines = [firstline]
        # TODO(jelmer): What if there are multiple targets?
        self.target = firstline.split(b':')[0]

    def rename_target(self, oldname, newname):
        # TODO(jelmer): Handle multiple targets
        if self.target == oldname:
            self.target = newname
            self.firstline = b';'.join([
                self.target, self.lines[0].split(b':', 1)[1]])
            return True
        return False

    def commands(self):
        return [l[1:] for l in self.lines if l.startswith(b'\t')]

    def append_line(self, line):
        self.lines.append(line)

    def dump_lines(self):
        return self.lines

    def __bool__(self):
        return bool(self.lines)

    def clear(self):
        self.lines = []


def update_makefile(path, command_line_cb=None, global_line_cb=None,
                    rule_cb=None):
    """Update a makefile.

    Args:
      path: Path to the makefile to edit
      command_line_cb: Callback to call on every rule command line
      global_line_cb: Callback to call on every global line
      rule_cb: Callback called for every rule
    Returns:
      boolean indicating whether any changes were made
    """
    if not os.path.exists(path):
        return False
    with open(path, 'rb') as f:
        original_contents = f.read()
    newlines = []
    rule = None

    def process_rule():
        if rule:
            if rule_cb:
                rule_cb(rule)
            if not rule:
                return False
            newlines.extend(rule.dump_lines())
            return True
        else:
            return False

    keep = b''
    for line in original_contents.splitlines():
        line = keep + line
        keep = b''
        if line.endswith(b'\\'):
            keep = line + b'\n'
            continue
        if line.startswith(b'\t') and rule:
            ret = line[1:]
            if callable(command_line_cb):
                ret = command_line_cb(ret, rule.target)
            elif isinstance(command_line_cb, list):
                for fn in command_line_cb:
                    ret = fn(ret, rule.target)
            if isinstance(ret, bytes):
                rule.append_line(b'\t' + ret)
            elif isinstance(ret, list):
                for l in ret:
                    rule.append_line(b'\t' + l)
            else:
                raise TypeError(ret)
        elif b':' in line and b' ' not in line.split(b':')[0]:
            process_rule()
            rule = Rule(line)
        elif not line.strip():
            if rule:
                rule.append_line(line)
            else:
                newlines.append(line)
        else:
            process_rule()
            rule = None

            if global_line_cb:
                line = global_line_cb(line)
            if line is None:
                pass
            elif isinstance(line, list):
                newlines.extend(line)
            elif isinstance(line, bytes):
                newlines.append(line)
            else:
                raise TypeError(line)

    if keep:
        raise ValueError('file ends with continuation line')

    if rule:
        if not process_rule():
            while newlines and not newlines[-1].strip():
                del newlines[-1]

    updated_contents = b''.join([l+b'\n' for l in newlines])
    if updated_contents.strip() != original_contents.strip():
        with open(path, 'wb') as f:
            f.write(updated_contents)
        return True
    return False


def discard_pointless_override(rule):
    if not rule.target.startswith(b'override_'):
        return
    command = rule.target[len(b'override_'):]
    if rule.commands() == [command]:
        rule.clear()


def update_rules(command_line_cb=None, global_line_cb=None,
                 rule_cb=discard_pointless_override, path='debian/rules'):
    """Update a debian/rules file.

    Args:
      command_line_cb: Callback to call on every rule command line
      global_line_cb: Callback to call on every global line
      rule_cb: Callback to call on every rule
      path: Path to the debian/rules file to edit
    Returns:
      boolean indicating whether any changes were made
    """
    return update_makefile(
        path, command_line_cb=command_line_cb, global_line_cb=global_line_cb,
        rule_cb=rule_cb)


def dh_invoke_drop_with(line, with_argument):
    """Drop a particular value from a with argument."""
    if with_argument not in line:
        return line
    # It's the only with argument
    line = re.sub(b" --with[ =]" + with_argument + b"( .+|)$", b"\\1", line)
    # It's at the beginning of the line
    line = re.sub(b" --with[ =]" + with_argument + b",", b" --with=", line)
    # It's somewhere in the middle or at the end
    line = re.sub(
        b" --with[ =]([^,]+)," + with_argument + b"([ ,])",
        b" --with=\\1\\2", line)
    # It's at the end
    line = re.sub(
        b" --with[ =](.+)," + with_argument + b"$",
        b" --with=\\1", line)
    return line


def dh_invoke_drop_argument(line, argument):
    """Drop a particular argument from a dh invocation."""
    if argument not in line:
        return line
    line = re.sub(b' ' + argument + b'$', b'', line)
    line = re.sub(b' ' + argument + b' ', b' ', line)
    return line


def dh_invoke_replace_argument(line, old, new):
    if old not in line:
        return line
    line = re.sub(b' ' + old + b'$', b' ' + new, line)
    line = re.sub(b' ' + old + b' ', b' ' + new + b' ', line)
    return line


def check_cdbs(path='debian/rules'):
    if not os.path.exists(path):
        return False
    with open(path, 'rb') as f:
        for line in f:
            if line.lstrip(b'-').startswith(b'include /usr/share/cdbs/'):
                return True
    return False
