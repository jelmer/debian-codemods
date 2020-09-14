#!/usr/bin/python3

import asyncio
import sys

try:
    import asyncpg  # noqa: F401
except ModuleNotFoundError:
    sys.exit(2)

from debmutate.control import ControlEditor
from debmutate.vcs import (
    split_vcs_url, unsplit_vcs_url,
    )
from lintian_brush.fixer import (
    report_result, fixed_lintian_tag, diligence, warn,
    )
from lintian_brush.vcs import canonicalize_vcs_url, determine_browser_url


if diligence() < 1:
    # Removing unused overrides requires pro-actively contacting UDD.
    sys.exit(0)

from lintian_brush.vcswatch import VcsWatch, VcsWatchError


async def find_branch(vcs_type, url):
    vcswatch = VcsWatch()
    await vcswatch.connect()
    return await vcswatch.get_branch_from_url(vcs_type, vcs_git)


with ControlEditor() as updater:
    try:
        vcs_git = updater.source['Vcs-Git']
    except KeyError:
        pass
    else:
        repo_url, branch, subpath = split_vcs_url(vcs_git)
        loop = asyncio.get_event_loop()
        try:
            new_branch = loop.run_until_complete(find_branch('Git', vcs_git))
        except KeyError:
            pass
        except VcsWatchError as e:
            warn('vcswatch URL unusable: %s' % e.args[0])
        else:
            if branch != new_branch:
                updater.source['Vcs-Git'] = unsplit_vcs_url(
                    repo_url, new_branch, subpath)
                vcs_browser = determine_browser_url(
                    'Git', updater.source['Vcs-Git'])
                if vcs_browser is not None:
                    updater.source['Vcs-Browser'] = vcs_browser
                report_result("Set branch from vcswatch in Vcs-Git URL.")
