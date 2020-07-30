#!/usr/bin/python3

from lintian_brush.fixer import report_result
from lintian_brush.yaml import YamlUpdater
from ruamel.yaml.reader import ReaderError  # noqa: E402

import sys

try:
    with YamlUpdater('debian/upstream/metadata') as editor:
        if isinstance(editor.code, dict):
            sys.exit(0)

        if isinstance(editor.code, list):
            if len(editor.code) == 1:
                editor.code = editor.code[0]
                report_result(
                    'Use YAML mapping in debian/upstream/metadata.',
                    fixed_lintian_tags=['upstream-metadata-not-yaml-mapping'])
            elif all([
                    isinstance(m, dict) and len(m) == 1
                    for m in editor.code]):
                old = editor.code
                editor.code = {}
                for entry in old:
                    editor.code.update(entry)
                report_result(
                    'Use YAML mapping in debian/upstream/metadata.',
                    fixed_lintian_tags=['upstream-metadata-not-yaml-mapping'])
except FileNotFoundError:
    sys.exit(0)
except ReaderError:
    # Maybe attempt to convert to YAML somehow
    sys.exit(0)
