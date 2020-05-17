#!/usr/bin/python3
from lintian_brush.fixer import (
    report_result,
    opinionated,
    package_is_native,
    )
import os
import sys

tags = []
description = None

if not os.path.exists('debian/source/format'):
    orig_format = None
    format = '1.0'
    tags.append('missing-debian-source-format')
    description = "Explicitly specify source format."
else:
    with open('debian/source/format', 'r') as f:
        format = orig_format = f.read().strip()

if orig_format not in (None, '1.0'):
    sys.exit(0)

if package_is_native():
    format = '3.0 (native)'
    description = "Upgrade to newer source format %s." % format
else:
    from lintian_brush.patches import (
        tree_non_patches_changes,
        find_patches_directory,
        )
    from breezy import errors
    from breezy.workingtree import WorkingTree
    patches_directory = find_patches_directory('.')
    if patches_directory not in ('debian/patches', None):
        # Non-standard patches directory.
        sys.stderr.write(
            'Tree has non-standard patches directory %s.\n' % (
                patches_directory))
    else:
        try:
            tree, path = WorkingTree.open_containing('.')
        except errors.NotBranchError:
            # TODO(jelmer): Or maybe don't do anything ?
            format = "3.0 (quilt)"
            description = "Upgrade to newer source format %s." % format
        else:
            delta = list(tree_non_patches_changes(tree, patches_directory))
            if delta:
                sys.stderr.write(
                    'Tree has non-quilt changes against upstream.\n')
                if opinionated():
                    format = "3.0 (quilt)"
                    description = "Upgrade to newer source format %s." % format
                    try:
                        with open('debian/source/options', 'r') as f:
                            options = list(f.readlines())
                    except FileNotFoundError:
                        options = []
                    if 'single-debian-patch\n' not in options:
                        options.append('single-debian-patch\n')
                        description = description.rstrip('.') + (
                            ', enabling single-debian-patch.')
                    if 'auto-commit\n' not in options:
                        options.append('auto-commit\n')
                    with open('debian/source/options', 'w') as f:
                        f.writelines(options)
            else:
                format = "3.0 (quilt)"
                description = "Upgrade to newer source format %s." % format

if not os.path.exists('debian/source'):
    os.mkdir('debian/source')

with open('debian/source/format', 'w') as f:
    f.write('%s\n' % format)

if format != '1.0':
    tags.append('older-source-format')

report_result(
    description=description,
    fixed_lintian_tags=tags)
