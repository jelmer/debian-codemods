#!/usr/bin/python3

from lintian_brush.rules import update_rules

certainty = None


def drop_get_orig_source(rule):
    global certainty
    if rule.has_target(b'get-orig-source'):
        commands = rule.commands()
        if [b'uscan'] == [c.split(b' ')[0] for c in commands]:
            certainty = 'certain'
        else:
            certainty = 'possible'
        rule.clear()


update_rules(rule_cb=drop_get_orig_source)
print('Remove unnecessary get-orig-source-target.')
print('Fixed-Lintian-Tags: '
      'debian-rules-contains-unnecessary-get-orig-source-target')
if certainty:
    print('Certainty: %s' % certainty)
