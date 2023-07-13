#!/usr/bin/python3

from lintian_brush.fixer import LintianIssue, control, report_result

binary_homepages = set()
source_homepage = None


with control as updater:
    source_homepage = updater.source.get('Homepage')
    for binary in updater.binaries:
        if 'Homepage' not in binary:
            continue
        if source_homepage == binary['Homepage']:
            issue = LintianIssue('source', 'homepage-in-binary-package')
            # Source and binary both have a homepage field, but they're the
            # same ⇒ drop the binary package Homepage field
            if issue.should_fix():
                issue.report_fixed()
                del binary['Homepage']
        else:
            binary_homepages.add(binary['Homepage'])

    if (source_homepage is None and binary_homepages
            and len(binary_homepages) == 1):
        updater.source['Homepage'] = binary_homepages.pop()

        for binary in updater.binaries:
            if 'Homepage' in binary:
                issue = LintianIssue(
                    'source', 'homepage-in-binary-package')
                if issue.should_fix():
                    del binary['Homepage']
                    issue.report_fixed()


report_result('Set Homepage field in Source rather than Binary package.')
