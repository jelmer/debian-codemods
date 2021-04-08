#!/usr/bin/python3

from urllib.parse import urlparse

from lintian_brush.fixer import (
    control,
    meets_minimum_certainty,
    report_result,
    trust_package,
    LintianIssue,
    )
from upstream_ontologist.guess import guess_upstream_metadata_items, known_bad_guess


def guess_homepage():
    for datum in guess_upstream_metadata_items(
            '.', trust_package=trust_package()):
        if datum.field != 'Homepage':
            continue
        if not meets_minimum_certainty(datum.certainty):
            continue
        if known_bad_guess(datum):
            continue
        return datum


with control as updater:
    if 'Homepage' not in updater.source:
        datum = guess_homepage()
        issue = LintianIssue('source', 'no-homepage-field')
        if datum and issue.should_fix():
            updater.source["Homepage"] = datum.value
            issue.report_fixed()
            report_result('Fill in Homepage field.', certainty=datum.certainty)
    else:
        if urlparse(updater.source['Homepage']).hostname == 'pypi.org':
            issue = LintianIssue('source', 'pypi-homepage', updater.source['Homepage'])
            datum = guess_homepage()
            if issue.should_fix() and datum:
                updater.source["Homepage"] = datum.value
                issue.report_fixed()
                report_result('Avoid pypi in Homepage field.', certainty=datum.certainty)
