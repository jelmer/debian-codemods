#!/usr/bin/python3
from debian.changelog import Version
import os
import sys
if not os.path.exists('debian/source/format'):
    format = '1.0'
    tag = 'missing-debian-source-format'
    description = "Explicitly specify source format."
else:
    tag = 'older-source-format'
    description = "Upgrade to newer source format."
    with open('debian/source/format', 'r') as f:
        format = f.read().strip()
        if format != '1.0':
            sys.exit(0)

version = Version(os.environ['CURRENT_VERSION'])

if not os.path.exists('debian/source'):
    os.mkdir('debian/source')

with open('debian/source/format', 'w') as f:
    if not version.debian_revision:
        f.write("3.0 (native)\n")
    else:
        f.write("3.0 (quilt)\n")
        from lintian_brush.patches import tree_non_patches_changes
        from breezy import errors
        from breezy.workingtree import WorkingTree
        try:
            tree, path = WorkingTree.open_containing('.')
        except errors.NotBranchError:
            # TODO(jelmer): Or maybe exit with code 2?
            pass
        else:
            delta = list(tree_non_patches_changes(tree))
            if delta:
                sys.stderr.write(
                    'Tree has non-quilt changes against upstream.\n')
                description = description.rstrip('.') + (
                    ', enabling single-debian-patch.')
                if os.environ.get('OPINIONATED', 'no') == 'yes':
                    try:
                        with open('debian/source/options', 'r') as f:
                            options = list(f.readlines())
                    except FileNotFoundError:
                        options = []
                    if 'single-debian-patch\n' not in options:
                        options.append('single-debian-patch\n')
                    if 'auto-commit\n' not in options:
                        options.append('auto-commit\n')
                    with open('debian/source/options', 'w') as f:
                        f.writelines(options)
                else:
                    # Let's leave it to the maintainer to convert.
                    sys.exit(2)


print(description)
print("Fixed-Lintian-Tags: %s" % tag)
