#!/usr/bin/python3

import sys
from typing import Any, Dict, List, Tuple

import ruamel.yaml.composer
from ruamel.yaml.nodes import MappingNode, SequenceNode
from ruamel.yaml.reader import ReaderError  # noqa: E402

from lintian_brush.fixer import fixed_lintian_tag, report_result
from lintian_brush.yaml import MultiYamlUpdater, YamlUpdater

SEQUENCE_FIELDS = ['Reference', 'Screenshots']

try:  # noqa: C901
    editor = YamlUpdater(
        'debian/upstream/metadata', allow_duplicate_keys=True)

    def flatten_mapping(node):
        if not isinstance(node, MappingNode):
            return
        by_key: Dict[Any, List[Tuple[int, Any]]] = {}
        for i, (k, v) in enumerate(node.value):
            by_key.setdefault(k.value, []).append((i, v))
        to_remove = []
        for k, vs in by_key.items():
            if len(vs) == 1:
                continue
            if k in SEQUENCE_FIELDS:
                if not isinstance(vs[0], SequenceNode):
                    node.value[vs[0][0]] = (
                        node.value[vs[0][0]][0],
                        SequenceNode(
                            tag='tag:yaml.org,2002:seq', value=[vs[0][1]],
                            start_mark=node.value[vs[0][0]][1].start_mark,
                            end_mark=node.value[vs[0][0]][1].end_mark))
                primary = node.value[vs[0][0]][1]
                for i, v in vs[1:]:
                    if isinstance(v, SequenceNode):
                        primary.value.extend(v.value)
                    elif isinstance(v, MappingNode):
                        primary.value.append(v)
                    else:
                        primary.value.append(v)
                    to_remove.append((i, k))
            else:
                # Preserve the first value.
                # TODO(jelmer): Make a more informed choice.
                for (i, _v) in vs[1:]:
                    to_remove.append((i, k))
        if not to_remove:
            return
        for i, _k in sorted(to_remove, reverse=True):
            editor.force_rewrite()
            del node.value[i]
        fixed_lintian_tag('source', 'upstream-metadata-yaml-invalid')
        report_result(
            'Remove duplicate values for fields %s '
            'in debian/upstream/metadata.' % ', '.join(
                [k for (i, k) in sorted(to_remove)]))

    editor.yaml.constructor.flatten_mapping = flatten_mapping
    try:
        with editor:
            if isinstance(editor.code, dict):
                pass
            elif isinstance(editor.code, list):
                if len(editor.code) == 1:
                    editor.code = editor.code[0]
                    fixed_lintian_tag(
                        'source', 'upstream-metadata-not-yaml-mapping')
                    report_result(
                        'Use YAML mapping in debian/upstream/metadata.')
                elif all(isinstance(m, dict)
                         and len(m) == 1 for m in editor.code):
                    old = editor.code
                    editor.code = {}
                    for entry in old:
                        editor.code.update(entry)
                        fixed_lintian_tag(
                            'source', 'upstream-metadata-not-yaml-mapping')
                    report_result(
                        'Use YAML mapping in debian/upstream/metadata.')
    except ruamel.yaml.composer.ComposerError:
        ranges = []
        with MultiYamlUpdater('debian/upstream/metadata') as multi_editor:
            for i, m in enumerate(multi_editor):
                if not m:
                    ranges.append(i)
            for i in reversed(ranges):
                del multi_editor[i]
        if ranges:
            report_result(
                'Discard extra empty YAML documents in '
                'debian/upstream/metadata.')


except FileNotFoundError:
    sys.exit(0)
except ReaderError:
    # Maybe attempt to convert to YAML somehow
    sys.exit(0)
