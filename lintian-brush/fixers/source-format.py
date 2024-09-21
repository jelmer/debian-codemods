#!/usr/bin/python3
import os
import sys

from lintian_brush.fixer import (
    LintianIssue,
    is_debcargo_package,
    meets_minimum_certainty,
    opinionated,
    package_is_native,
    report_result,
    warn,
)

if is_debcargo_package():
    sys.exit(0)

description = None

if not os.path.exists("debian/source/format"):
    orig_format = None
    format = "1.0"
    missing_source_format_issue = LintianIssue(
        "source", "missing-debian-source-format"
    )
    if not missing_source_format_issue.should_fix():
        sys.exit(0)
    missing_source_format_issue.report_fixed()
    description = "Explicitly specify source format."
else:
    with open("debian/source/format") as f:
        format = orig_format = f.read().strip()

if orig_format not in (None, "1.0"):
    sys.exit(0)

older_source_format_issue = LintianIssue(
    "source", "older-source-format", info=(orig_format or "1.0")
)

if older_source_format_issue.should_fix():
    if package_is_native():
        format = "3.0 (native)"
        description = f"Upgrade to newer source format {format}."
    else:
        from breezy import errors
        from breezy.workingtree import WorkingTree

        from lintian_brush.patches import (
            find_patches_directory,
            tree_has_non_patches_changes,
        )

        patches_directory = find_patches_directory(".")
        if patches_directory not in ("debian/patches", None):
            # Non-standard patches directory.
            warn(
                f"Tree has non-standard patches directory {patches_directory}."
            )
        else:
            try:
                tree, path = WorkingTree.open_containing(".")
            except errors.NotBranchError as e:
                if not meets_minimum_certainty("possible"):
                    warn(f"unable to open vcs to check for delta: {e}")
                    sys.exit(0)
                format = "3.0 (quilt)"
                description = f"Upgrade to newer source format {format}."
            else:
                delta = tree_has_non_patches_changes(tree, patches_directory)
                if delta:
                    warn("Tree has non-quilt changes against upstream.")
                    if opinionated():
                        format = "3.0 (quilt)"
                        description = (
                            f"Upgrade to newer source format {format}."
                        )
                        try:
                            with open("debian/source/options") as f:
                                options = list(f.readlines())
                        except FileNotFoundError:
                            options = []
                        if "single-debian-patch\n" not in options:
                            options.append("single-debian-patch\n")
                            description = description.rstrip(".") + (
                                ", enabling single-debian-patch."
                            )
                        if "auto-commit\n" not in options:
                            options.append("auto-commit\n")
                        with open("debian/source/options", "w") as f:
                            f.writelines(options)
                else:
                    format = "3.0 (quilt)"
                    description = f"Upgrade to newer source format {format}."

if not os.path.exists("debian/source"):
    os.mkdir("debian/source")

with open("debian/source/format", "w") as f:
    f.write(f"{format}\n")

if format != "1.0":
    older_source_format_issue.report_fixed()

report_result(description=description)
