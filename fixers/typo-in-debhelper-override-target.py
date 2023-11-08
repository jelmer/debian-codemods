#!/usr/bin/python3

import os
import sys
from typing import List, Tuple

from debmutate._rules import RulesEditor

from lintian_brush.fixer import LintianIssue, report_result
from lintian_brush.lintian import dh_commands

if not os.path.exists("debian/rules"):
    sys.exit(2)

try:
    from Levenshtein import distance
except ModuleNotFoundError:
    sys.exit(2)

known_dh_commands = list(dh_commands())

# Include javahelper binaries, since some are just one character away from
# debhelper ones.
known_dh_commands.extend(
    [
        "jh_build",
        "jh_classpath",
        "jh_clean",
        "jh_compilefeatures",
        "jh_depends",
        "jh_exec",
        "jh_generateorbitdir",
        "jh_installeclipse",
        "jh_installjavadoc",
        "jh_installlibs",
        "jh_linkjars",
        "jh_makepkg",
        "jh_manifest",
        "jh_repack",
        "jh_setupenvironment",
        "mh_checkrepo",
        "mh_install",
        "mh_installpoms",
        "mh_linkjars",
        "mh_patchpoms",
        "mh_clean",
        "mh_installjar",
        "mh_installsite",
        "mh_linkrepojar",
        "mh_unpatchpoms",
        "mh_cleanpom",
        "mh_installpom",
        "mh_linkjar",
        "mh_patchpom",
    ]
)

known_targets = set()
for dh_command in known_dh_commands:
    known_targets.update(
        [
            "override_" + dh_command,
            "execute_before_" + dh_command,
            "execute_after_" + dh_command,
        ]
    )


renamed: List[Tuple[str, str]] = []

with RulesEditor() as editor:
    for rule in editor.makefile.iter_all_rules():
        if rule.target.decode() in known_targets:
            continue
        for known_target in known_targets:
            issue = LintianIssue(
                "source",
                "typo-in-debhelper-override-target",
                "%s -> %s (line X)",
            )
            if (
                distance(known_target, rule.target.decode()) == 1
                and issue.should_fix()
            ):
                renamed.append((rule.target.decode(), known_target))
                rule.rename_target(rule.target, known_target.encode())
                issue.report_fixed()


report_result(
    "Fix typo in debian/rules rules: %s"
    % ", ".join(f"{old} ⇒ {new}" for old, new in renamed)
)
