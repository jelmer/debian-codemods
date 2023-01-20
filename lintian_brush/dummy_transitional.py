#!/usr/bin/python3
# Copyright (C) 2022 Jelmer Vernooij
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

import argparse
import asyncio
from dataclasses import dataclass
import logging
import re
import sys
from typing import Dict, Set
import warnings
import yaml

from debian.deb822 import PkgRelation
from debmutate.control import suppress_substvar_warnings


def parse_relations(t: str):
    with warnings.catch_warnings():
        suppress_substvar_warnings()
        return PkgRelation.parse_relations(t)


REGEXES = [
    r'.*\((.*, )?(dummy )?transitional (dummy )?package\)',
    r'.*\((.*, )?(dummy )?transitional (dummy )?package for ([^ ]+)\)',
    r'.*\(transitional development files\)',
    r'.*\(transitional\)',
    r'.* [-—] transitional( package)?',
    r'.*\[transitional package\]',
    r'.* - transitional (dummy )?package',
    r'transitional package -- safe to remove',
    r'(dummy )?transitional (dummy )?package (for|to) (.*)',
    r'transitional dummy package',
    r'transitional dummy package: ([^ ]+)',
    r'transitional package, ([^ ]+)',
    r'(dummy )?transitional (dummy )?package, ([^ ]+) to ([^ ]+)',
    r'transitional package( [^ ]+)?',
    r'([^ ]+) transitional package',
    r'.* transitional package',
    r'.*transitional package for .*',
]


@dataclass
class TransitionalPackage:

    from_name: str
    to_expr: str

    def json(self):
        return {'from_name': self.from_name, 'to_expr': self.to_expr}


async def find_reverse_dependencies(udd, package):
    by_source: Dict[str, Set[str]] = {}
    fields = ['recommends', 'depends', 'pre_depends', 'enhances',
              'suggests', 'provides']
    query = (
        "SELECT source, package, " + ", ".join(fields) +
        " FROM packages WHERE " + " OR ".join([
            f"{field} LIKE CONCAT('%', $1::text, '%')" for field in fields]))
    for row in await udd.fetch(query, package):
        for relname in fields:
            parsed = parse_relations(row[relname] or '')
            for option in parsed:
                for rel in option:
                    if rel['name'] == package:
                        by_source.setdefault(
                            row['source'], set()).add(row['package'])

    fields = ["build_depends", "build_depends_indep", "build_depends_arch",
              "build_conflicts", "build_conflicts_indep",
              "build_conflicts_arch"]
    query = ("SELECT source, " + ", ".join(fields) +
             " FROM sources WHERE " + " OR ".join([
                 f"{field} LIKE CONCAT('%', $1::text, '%')"
                 for field in fields]))
    for row in await udd.fetch(query, package):
        for relname in fields:
            parsed = parse_relations(row[relname] or '')
            for option in parsed:
                for rel in option:
                    if rel['name'] == package:
                        by_source.setdefault(row['source'], set())
    return by_source


async def find_dummy_transitional_packages(udd, release):
    ret = {}
    query = """\
SELECT package, description, depends FROM packages
WHERE release = $1 AND description LIKE '%transitional%'"""
    for row in await udd.fetch(query, release):
        for regex in REGEXES:
            if re.fullmatch(regex, row[1]):
                break
        else:
            logging.debug(
                'Unknown syntax for dummy package description: %r', row[1])
        if row[2] is None:
            logging.debug('no replacement for %s', row[0])
        else:
            depends = parse_relations(row[2])
            if len(depends) != 1:
                logging.debug(
                    'no single transition target for %s: %r', row[0], row[2])
        ret[row[0]] = TransitionalPackage(
            from_name=row[0], to_expr=row[2])  # type: ignore
    return ret


async def main(argv=None):
    from .udd import connect_udd_mirror
    parser = argparse.ArgumentParser(prog="candidates")
    parser.add_argument("--release", type=str, default="sid")
    parser.add_argument('--list-transitional-dummy', action="store_true")
    parser.add_argument('--list-uses-transitional-dummy', action="store_true")
    args = parser.parse_args()

    logging.basicConfig(level=logging.INFO, format='%(message)s')

    async with await connect_udd_mirror() as udd:
        transitions = await find_dummy_transitional_packages(udd, args.release)

        if args.list_transitional_dummy:
            yaml.dump(
                [transition.json() for transition in transitions.values()],
                sys.stdout)
        elif args.list_uses_transitional_dummy:
            for dep in transitions:
                by_source = await find_reverse_dependencies(udd, dep)
                for source, binaries in by_source.items():
                    for binary in binaries:
                        print(f'{source} / {binary} / {dep}')
        else:
            parser.print_usage()
            return 1


if __name__ == '__main__':
    sys.exit(asyncio.run(main(sys.argv[1:])))
