#!/usr/bin/python3

from lintian_brush.fixer import report_result, meets_minimum_certainty

import os
import sys

CERTAINTY = 'possible'

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)

if os.path.exists('debian/copyright'):
    sys.exit(0)


try:
    import decopy  # noqa: F401
except ModuleNotFoundError:
    # No decopy
    sys.exit(2)

from decopy.decopy import prepare_output_groups
from decopy.tree import RootInfo, DirInfo
from decopy.dep5 import Copyright, Group
from decopy.output import generate_output
from decopy.cmdoptions import process_options

options = process_options(['--root=.', '--no-progress', '--mode=full', '--output=debian/copyright'])

filetree = RootInfo.build(options)
copyright_ = Copyright.build(filetree, options)

copyright_.process(filetree)
filetree.process(options)

groups = prepare_output_groups(filetree, copyright_, options)

generate_output(groups, filetree, copyright_, options)

report_result(
    'Create a debian/copyright file.',
    certainty=CERTAINTY,
    fixed_lintian_tags=['no-copyright-file'])
