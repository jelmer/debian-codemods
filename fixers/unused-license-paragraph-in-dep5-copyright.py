#!/usr/bin/python3

from debmutate.copyright import CopyrightEditor, NotMachineReadableError
from lintian_brush.fixer import report_result
from lintian_brush.lintian_overrides import override_exists
import sys
import re

used = []
defined = set()
certainty = 'certain'


def extract_licenses(synopsis):
    """Extract license names from a synopsis.

    This will return a list of licenses, as a list of possible names per
    license.
    """
    ret = []
    for license in synopsis.split(" or "):
        options = []
        options.append(license)
        m = re.fullmatch(r'(.*) with (.*) exception', license)
        if m:
            license = m.group(1)
        options.append(license)
        ret.append(options)
    return ret


try:
    with CopyrightEditor() as updater:
        if updater.copyright.header.license:
            if updater.copyright.header.license.text:
                defined.add(updater.copyright.header.license.synopsis)
        for paragraph in updater.copyright.all_paragraphs():
            if not paragraph.license:
                continue
            if paragraph.license.text:
                defined.add(paragraph.license.synopsis)

        if updater.copyright.header.license:
            synopsis = updater.copyright.header.license.synopsis
            if synopsis:
                if synopsis in defined:
                    used.append([synopsis])
                used.extend(extract_licenses(synopsis))

        for paragraph in updater.copyright.all_files_paragraphs():
            if not paragraph.license:
                continue
            if paragraph.files:
                synopsis = paragraph.license.synopsis
                if synopsis in defined:
                    used.append([synopsis])
                used.extend(extract_licenses(synopsis))

        extra_defined = set(defined)
        for options in used:
            extra_defined -= set(options)

        extra_used = []
        for options in used:
            for option in options:
                if option in defined:
                    break
            else:
                extra_used.append(options)

        if extra_used:
            sys.stderr.write('Undefined licenses in copyright: %r' %
                             [options[0] for options in extra_used])
            # Drop the certainty since it's possible the undefined licenses
            # are actually the referenced ones.
            certainty = 'possible'

        for name in extra_defined:
            for paragraph in updater.copyright.all_paragraphs():
                if not paragraph.license:
                    continue
                if paragraph.license.synopsis == name:
                    continue
                if paragraph.license.text and name in paragraph.license.text:
                    certainty = 'possible'
                if paragraph.comment and name in paragraph.comment:
                    certainty = 'possible'

        if extra_defined and not extra_used:
            for paragraph in list(updater.copyright._Copyright__paragraphs):
                if not paragraph.license:
                    continue
                if override_exists(
                        'unused-license-paragraph-in-dep5-copyright',
                        package='source',
                        info=paragraph.license.synopsis.lower()):
                    continue
                if paragraph.license.synopsis in extra_defined:
                    updater.copyright._Copyright__paragraphs.remove(paragraph)
except (FileNotFoundError, NotMachineReadableError):
    pass
else:
    report_result(
        "Remove unused license definitions for %s." % ', '.join(extra_defined),
        fixed_lintian_tags=['unused-license-paragraph-in-dep5-copyright'],
        certainty=certainty)
