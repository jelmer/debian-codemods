#!/usr/bin/python3

from lintian_brush.fixer import report_result, fixed_lintian_tag
from lintian_brush.yaml import YamlUpdater
from ruamel.yaml.reader import ReaderError  # noqa: E402
from ruamel.yaml.nodes import MappingNode

import sys

try:
    editor = YamlUpdater(
            'debian/upstream/metadata',
            allow_duplicate_keys=True)

    def flatten_mapping(node):
        if not isinstance(node, MappingNode):
            return
        by_key = {}
        for i, (k, v) in enumerate(node.value):
            by_key.setdefault(k.value, []).append((i, v))
        to_remove = []
        for (k, vs) in by_key.items():
            if len(vs) == 1:
                continue
            # Preserve the first value.
            # TODO(jelmer): Make a more informed choice.
            for (i, v) in vs[1:]:
                to_remove.append((i, k))
        if not to_remove:
            return
        for i, k in sorted(to_remove, reverse=True):
            editor.force_rewrite()
            del node.value[i]
        fixed_lintian_tag('source', 'upstream-metadata-yaml-invalid')
        report_result(
            'Remove duplicate values for fields %s '
            'in debian/upstream/metadata.' % ', '.join(
                [k for (i, k) in sorted(to_remove)]))

    editor.yaml.constructor.flatten_mapping = flatten_mapping
    with editor:
        if isinstance(editor.code, dict):
            pass
        elif isinstance(editor.code, list):
            if len(editor.code) == 1:
                editor.code = editor.code[0]
                fixed_lintian_tag(
                    'source', 'upstream-metadata-not-yaml-mapping')
                report_result('Use YAML mapping in debian/upstream/metadata.')
            elif all([
                    isinstance(m, dict) and len(m) == 1
                    for m in editor.code]):
                old = editor.code
                editor.code = {}
                for entry in old:
                    editor.code.update(entry)
                    fixed_lintian_tag(
                        'source', 'upstream-metadata-not-yaml-mapping')
                report_result('Use YAML mapping in debian/upstream/metadata.')
except FileNotFoundError:
    sys.exit(0)
except ReaderError:
    # Maybe attempt to convert to YAML somehow
    sys.exit(0)
