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


def wildcard_to_re(wildcard):
    wc = []
    for c in wildcard:
        if c == '%':
            wc.append('.*')
        else:
            wc.append(re.escape(c))
    return re.compile(''.join(wc))


def matches_wildcard(text, wildcard):
    wc = wildcard_to_re(wildcard)
    return wc.fullmatch(text)


class Rule(object):
    """A make rule."""

    def __init__(self, target=None, commands=None, prereq_targets=None):
        self.target = target
        self.components = prereq_targets or []
        if self.components:
            self._component_str = b' ' + b' '.join(self.components)
        else:
            self._component_str = b''
        if target:
            self.lines = (
                [b'%s:%s' % (target, self._component_str)] +
                [b'\t' + cmd for cmd in (commands or [])])
        else:
            self.lines = None

    @classmethod
    def _from_first_line(cls, firstline):
        self = cls()
        self.lines = [firstline]
        # TODO(jelmer): What if there are multiple targets?
        self.target, self._component_str = firstline.split(b':', 1)
        self.components = self._component_str.split()
        return self

    def __repr__(self):
        return "<%s(%r)>" % (type(self).__name__, self.target)

    def has_target(self, target, exact=True):
        if exact:
            # TODO(jelmer): Handle multiple targets
            return self.target == target
        else:
            # TODO(jelmer): Handle multiple targets
            return matches_wildcard(target.decode(), self.target.decode())

    def rename_target(self, oldname, newname):
        # TODO(jelmer): Handle multiple targets
        if self.target == oldname:
            self.target = newname
            firstline = b':'.join([self.target, self._component_str])
            self.lines = [firstline] + self.lines[1:]
            return True
        return False

    def commands(self):
        return [l[1:] for l in self.lines if l.startswith(b'\t')]

    def append_line(self, line):
        self.lines.append(line)

    def append_command(self, command):
        self.lines.append(b'\t' + command + b'\n')

    def append_component(self, component):
        self.components.append(component)
        self._component_str = b' ' + b' '.join(self.components)
        self.lines[0] = b'%s:%s' % (self.target, self._component_str)

    def dump_lines(self):
        return [line + b'\n' for line in self.lines]

    def __eq__(self, other):
        if not isinstance(other, type(self)):
            return False
        return self.dump_lines() == other.dump_lines()

    def __bool__(self):
        return bool(self.lines)

    def clear(self):
        self.lines = []

    def _trim_trailing_whitespace(self):
        while self.lines and not self.lines[-1].strip():
            del self.lines[-1]

    def _ensure_trailing_whitespace(self):
        if self.lines and self.lines[-1].strip():
            self.lines.append(b'')


def is_conditional(line):
    line = line.lstrip(b' ')
    return (
        line.startswith(b'ifeq') or
        line.startswith(b'ifneq') or
        line.startswith(b'else') or
        line.startswith(b'endif') or
        line.startswith(b'include') or
        line.startswith(b'-include'))


class Makefile(object):

    def __init__(self, contents=None):
        self.contents = list(contents or [])

    @classmethod
    def from_path(cls, path):
        with open(path, 'rb') as f:
            original_contents = f.read()
        return cls.from_bytes(original_contents)

    def iter_rules(self, target, exact=True):
        for entry in self.contents:
            if isinstance(entry, Rule) and entry.has_target(target, exact):
                yield entry
        else:
            return

    @classmethod
    def from_bytes(cls, contents):
        mf = cls()
        keep = b''
        rule = None
        joinedlines = []
        for line in contents.splitlines():
            line = keep + line
            keep = b''
            if line.endswith(b'\\'):
                keep = line + b'\n'
                continue
            joinedlines.append(line)

        if keep:
            # file ends with continuation line..
            joinedlines.append(keep)

        for line in joinedlines:
            if line.startswith(b'\t') and rule:
                rule.append_line(line)
            elif is_conditional(line) or line.lstrip(b' ').startswith(b'#'):
                if rule:
                    rule.append_line(line)
                else:
                    mf.contents.append(line)
            elif b':' in line and b' ' not in line.split(b':')[0]:
                if rule:
                    mf.contents.append(rule)
                rule = Rule._from_first_line(line)
            elif not line.strip():
                if rule:
                    rule.append_line(line)
                else:
                    mf.contents.append(line)
            else:
                if rule:
                    mf.contents.append(rule)
                rule = None
                mf.contents.append(line)

        if rule:
            mf.contents.append(rule)

        return mf

    def dump_lines(self):
        lines = []
        for entry in self.contents:
            if isinstance(entry, Rule):
                lines.extend(entry.dump_lines())
            else:
                lines.append(entry + b'\n')
        return lines

    def dump(self):
        return b''.join(self.dump_lines())

    def add_rule(self, target, components=None):
        if self.contents:
            if isinstance(self.contents[-1], Rule):
                self.contents[-1]._ensure_trailing_whitespace()
            elif self.contents[-1].strip():
                self.contents.append(b'\n')
        if isinstance(target, list):
            target = b' '.join(target)
        line = b'%s:' % target
        if components:
            line += b' ' + b' '.join(components)
        rule = Rule._from_first_line(line)
        self.contents.append(rule)
        return rule


def update_makefile(path, command_line_cb=None, global_line_cb=None,
                    rule_cb=None, makefile_cb=None):
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
    mf = Makefile.from_bytes(original_contents)

    newcontents = []
    for entry in mf.contents:
        if isinstance(entry, Rule):
            rule = entry
            newlines = []
            for line in list(rule.lines[1:]):
                if line.startswith(b'\t'):
                    ret = line[1:]
                    if callable(command_line_cb):
                        ret = command_line_cb(ret, rule.target)
                    elif isinstance(command_line_cb, list):
                        for fn in command_line_cb:
                            ret = fn(ret, rule.target)
                    if isinstance(ret, bytes):
                        newlines.append(b'\t' + ret)
                    elif isinstance(ret, list):
                        for l in ret:
                            newlines.append(b'\t' + l)
                    else:
                        raise TypeError(ret)
                else:
                    newlines.append(line)

            rule.lines = [rule.lines[0]] + newlines

            if rule_cb:
                rule_cb(rule)
            if rule:
                newcontents.append(rule)
        else:
            line = entry
            if global_line_cb:
                line = global_line_cb(line)
            if line is None:
                pass
            elif isinstance(line, list):
                newcontents.extend(line)
            elif isinstance(line, bytes):
                newcontents.append(line)
            else:
                raise TypeError(line)

    if newcontents and isinstance(newcontents[-1], Rule):
        newcontents[-1]._trim_trailing_whitespace()

    mf = Makefile(newcontents)
    if makefile_cb:
        makefile_cb(mf)

    updated_contents = mf.dump()
    if updated_contents.strip() != original_contents.strip():
        updated_contents = updated_contents.rstrip(b'\n')
        if updated_contents:
            updated_contents += b'\n'
        with open(path, 'wb') as f:
            f.write(updated_contents)
        return True
    return False


def discard_pointless_override(rule):
    if not rule.target.startswith(b'override_'):
        return
    command = rule.target[len(b'override_'):]
    if rule.commands() == [command] and not rule.components:
        rule.clear()


def update_rules(command_line_cb=None, global_line_cb=None,
                 rule_cb=None,
                 makefile_cb=None, path='debian/rules'):
    """Update a debian/rules file.

    Args:
      command_line_cb: Callback to call on every rule command line
      global_line_cb: Callback to call on every global line
      rule_cb: Callback to call on every rule
      path: Path to the debian/rules file to edit
    Returns:
      boolean indicating whether any changes were made
    """
    changed = update_makefile(
        path, command_line_cb=command_line_cb, global_line_cb=global_line_cb,
        rule_cb=rule_cb, makefile_cb=makefile_cb)
    if changed:
        update_makefile(path, rule_cb=discard_pointless_override)
        return True
    else:
        return False


def dh_invoke_add_with(line, with_argument):
    """Add a particular value to a with argument."""
    if with_argument in line:
        return line
    if b' --with' not in line:
        return line + b' --with=' + with_argument
    return re.sub(
        b'([ \t])--with([ =])([^ \t]+)', b'\\1--with\\2\\3,' + with_argument,
        line)


def dh_invoke_drop_with(line, with_argument):
    """Drop a particular value from a with argument."""
    if with_argument not in line:
        return line
    # It's the only with argument
    line = re.sub(
        b"[ \t]--with[ =]" + with_argument + b"( .+|)$",
        b"\\1", line)
    # It's at the beginning of the line
    line = re.sub(
        b"([ \t])--with([ =])" + with_argument + b",",
        b"\\1--with\\2", line)
    # It's somewhere in the middle or at the end
    line = re.sub(
        b"([ \t])--with[ =]([^,]+)," + with_argument + b"([ ,])",
        b"\\1--with=\\2\\3", line)
    # It's at the end
    line = re.sub(
        b"([ \t])--with[ =](.+)," + with_argument + b"$",
        b"\\1--with=\\2", line)
    return line


def dh_invoke_drop_argument(line, argument):
    """Drop a particular argument from a dh invocation."""
    if argument not in line:
        return line
    line = re.sub(b'[ \t]+' + argument + b'$', b'', line)
    line = re.sub(b'([ \t])' + argument + b'[ \t]', b'\\1', line)
    return line


def dh_invoke_replace_argument(line, old, new):
    if old not in line:
        return line
    line = re.sub(b'([ \t])' + old + b'$', b'\\1' + new, line)
    line = re.sub(
        b'([ \t])' + old + b'([ \t])', b'\\1' + new + b'\\2', line)
    return line


def check_cdbs(path='debian/rules'):
    if not os.path.exists(path):
        return False
    with open(path, 'rb') as f:
        for line in f:
            if line.lstrip(b'-').startswith(b'include /usr/share/cdbs/'):
                return True
    return False
