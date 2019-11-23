#!/usr/bin/python3

from lintian_brush.rules import update_rules


def drop_get_orig_source(rule):
    if rule.has_target(b'get-orig-source'):
        rule.clear()


update_rules(rule_cb=drop_get_orig_source)
print('Remove unnecessary get-orig-source-target.')
print('Fixed-Lintian-Tags: '
      'debian-rules-contains-unnecessary-get-orig-source-target')
