#!/usr/bin/python3

from debmutate.watch import WatchEditor
from urllib.parse import urlparse

from lintian_brush.fixer import report_result, LintianIssue


with WatchEditor() as updater:
    for w in getattr(updater.watch_file, "entries", []):
        parsed_url = urlparse(w.url)

        # only applies to github.com
        if parsed_url.netloc != "github.com":
            continue

        # when searching /org/repo/tags
        parts = parsed_url.path.strip("/").split("/")
        if parts[-1] != "tags":
            continue

        # matching pattern contains /archive/
        if (
            "/archive/" in w.matching_pattern
            and not "/archive/refs/tags/" in w.matching_pattern
        ):
            issue = LintianIssue(
                "source",
                "github-archive-url",
                info="%s %s" % (w.url, w.matching_pattern),
            )
            if issue.should_fix():
                w.matching_pattern = w.matching_pattern.replace(
                    "/archive/", "/archive/refs/tags/"
                )
                issue.report_fixed()


report_result(
    "Fix changes to github archive URLs from the /<org>/<repo>/tags page"
    "/<org>/<repo>/archive/<tag> -> /<org>/<repo>/archive/refs/tags/<tag>"
)
