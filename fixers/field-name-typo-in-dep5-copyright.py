#!/usr/bin/python3

import sys
from contextlib import suppress

from debmutate.deb822 import Deb822Editor
from lintian_brush.fixer import fixed_lintian_tag, report_result, warn

try:
    from Levenshtein import distance
except ImportError:
    sys.exit(2)

valid_field_names = {
    "Files",
    "License",
    "Copyright",
    "Comment",
    "Upstream-Name",
    "Format",
    "Upstream-Contact",
    "Source",
    "Upstream",
    "Contact",
    "Name",
}

typo_fixed = set()
case_fixed = set()

with suppress(FileNotFoundError), Deb822Editor("debian/copyright") as updater:
    for paragraph in updater.paragraphs:
        for field in paragraph:
            if field in valid_field_names:
                continue
            if field.startswith("X-") and field[2:] in valid_field_names:
                if field[2:] in paragraph:
                    warn(f"Both {field} and {field[2:]} exist.")
                    continue
                value = paragraph[field]
                del paragraph[field]
                paragraph[field[2:]] = value
                typo_fixed.add((field, field[2:]))
                fixed_lintian_tag(
                    "source",
                    "field-name-typo-in-dep5-copyright",
                    f"{field} (line XX)",
                )
                continue

            for option in valid_field_names:
                if distance(field, option) == 1:
                    value = paragraph[field]
                    if option in paragraph and option.lower() != field.lower():
                        warn(
                            f"Found typo ({field} ⇒ {option}), "
                            f"but {option} already exists"
                        )
                        continue
                    del paragraph[field]
                    paragraph[option] = value
                    if option.lower() == field.lower():
                        case_fixed.add((field, option))
                    else:
                        typo_fixed.add((field, option))
                        fixed_lintian_tag(
                            "source",
                            "field-name-typo-in-dep5-copyright",
                            f"{field} (line XX)",
                        )
                    break


if case_fixed:
    kind = "case" + ("s" if len(case_fixed) > 1 else "")
else:
    kind = ""
if typo_fixed:
    if case_fixed:
        kind += " and "
    kind += "typo" + ("s" if len(typo_fixed) > 1 else "")

fixed_str = ", ".join(
    [
        f"{old} ⇒ {new}"
        for (old, new) in sorted(list(case_fixed) + list(typo_fixed))
    ]
)

report_result(f"Fix field name {kind} in debian/copyright ({fixed_str}).")
