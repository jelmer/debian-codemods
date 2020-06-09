#!/usr/bin/python3

from lintian_brush.rules import RulesEditor
from lintian_brush.fixer import report_result

certainty = None

with RulesEditor() as updater:
    for rule in updater.makefile.iter_rules(b'get-orig-source'):
        commands = rule.commands()
        if [b'uscan'] == [c.split(b' ')[0] for c in commands]:
            certainty = 'certain'
        else:
            certainty = 'possible'
        rule.clear()
        updater.makefile.drop_phony(b'get-orig-source')


report_result(
    'Remove unnecessary get-orig-source-target.',
    fixed_lintian_tags=[
        'debian-rules-contains-unnecessary-get-orig-source-target'],
    certainty=certainty)
