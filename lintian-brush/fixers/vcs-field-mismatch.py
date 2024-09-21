#!/usr/bin/python3

from urllib.parse import urlparse

from lintian_brush.fixer import control, fixed_lintian_tag, report_result

HOST_TO_VCS = {
    "github.com": "Git",
    "gitlab.com": "Git",
    "salsa.debian.org": "Git",
}


with control as updater:
    for field in list(updater.source):
        if not field.startswith("Vcs-") or field.lower() == "vcs-browser":
            continue
        vcs = field[4:]
        vcs_url = updater.source[field]
        host = urlparse(vcs_url).netloc.split("@")[-1]
        if host not in HOST_TO_VCS:
            continue
        actual_vcs = HOST_TO_VCS[host]
        if actual_vcs != vcs:
            fixed_lintian_tag(
                "source",
                "vcs-field-mismatch",
                f"{field} != Vcs-{actual_vcs} {vcs_url}",
            )
            del updater.source["Vcs-" + vcs]
            updater.source["Vcs-" + actual_vcs] = vcs_url
            report_result(
                f"Changed vcs type from {vcs} to {actual_vcs} based on URL."
            )
