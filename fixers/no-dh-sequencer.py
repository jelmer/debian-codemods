#!/usr/bin/python3

from lintian_brush.rules import update_rules


def use_dh_at_sequencer(line, target):
    if line.startswith(b'dh '):
        args = line.split(b' ')
        for possible_va in [b'$*', b'${*}']:
            try:
                x = args.index(possible_va)
            except ValueError:
                continue
            break
        else:
            return line
        args.pop(x)
        args.insert(1, b'$@')
        return b' '.join(args)


update_rules(use_dh_at_sequencer)
print('Use dh $@ sequencer.')
print('Fixed-Lintian-Tags: no-dh-sequencer')
