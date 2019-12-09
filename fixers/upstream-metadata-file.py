#!/usr/bin/python3

# TODO(jelmer): Read python3 setup.py dist_info
# TODO(jelmer): Check XS-Go-Import-Path

from debian.changelog import Version
import os
import sys
from lintian_brush import (
    certainty_sufficient,
    min_certainty,
    )
from lintian_brush.upstream_metadata import (
    UpstreamDatum,
    check_upstream_metadata,
    extend_upstream_metadata,
    fix_upstream_metadata,
    guess_upstream_metadata_items,
    update_from_guesses,
    filter_bad_guesses,
    ADDON_ONLY_FIELDS,
    )
from lintian_brush.yaml import YamlUpdater


current_version = Version(os.environ['CURRENT_VERSION'])


if not current_version.debian_revision:
    # Native package
    sys.exit(0)


with YamlUpdater('debian/upstream/metadata') as code:
    upstream_metadata = {
        k: UpstreamDatum(k, v, 'certain') for (k, v) in code.items()}

    minimum_certainty = os.environ.get('MINIMUM_CERTAINTY')
    trust_package = os.environ.get('TRUST_PACKAGE') == 'true'
    net_access = os.environ.get('NET_ACCESS', 'disallow') == 'allow'

    # Do some guessing based on what's in the package
    update_from_guesses(
        upstream_metadata, filter_bad_guesses(
            guess_upstream_metadata_items(
                '.', trust_package=trust_package)))

    # Then extend that by contacting e.g. SourceForge
    extend_upstream_metadata(
        upstream_metadata, '.',
        # Downgrade minimum certainty, since check_upstream_metadata can
        # upgrade it to "certain" later.
        minimum_certainty=(
            'likely'
            if net_access and minimum_certainty == 'certain'
            else minimum_certainty),
        net_access=net_access,
        consult_external_directory=True)
    if net_access:
        # Verify that online resources actually exist and adjust certainty
        # accordingly.
        check_upstream_metadata(
            upstream_metadata, version=current_version.upstream_version)

    # Homepage is set in debian/control, so don't add it to
    # debian/upstream/metadata.
    external_present_fields = set(['Homepage'])

    # If the debian/copyright file is machine-readable, then we do
    # not need to set the Name/Contact information in the
    # debian/upstream/metadata file.
    if 'Name' in upstream_metadata or 'Contact' in upstream_metadata:
        from lintian_brush.copyright import upstream_fields_in_copyright
        external_present_fields.update(upstream_fields_in_copyright())

    for key, datum in list(upstream_metadata.items()):
        # Drop keys that don't need to be in debian/upsteam/metadata
        if key.startswith('X-') or key in external_present_fields:
            del upstream_metadata[key]

        # Drop everything that is below our minimum certainty
        elif not certainty_sufficient(datum.certainty, minimum_certainty):
            del upstream_metadata[key]

    achieved_certainty = min_certainty(
        [d.certainty for d in upstream_metadata.values()])

    fix_upstream_metadata(upstream_metadata)

    changed = {
        k: v
        for k, v in upstream_metadata.items()
        if v.value != code.get(k)}

    if not changed:
        sys.exit(0)

    fixed_tag = not os.path.exists('debian/upstream/metadata')

    # If we're setting them new, put Name and Contact first
    def sort_key(x):
        (k, v) = x
        return {
            'Name': '00-Name',
            'Contact': '01-Contact',
            }.get(k, k)

    for k, v in sorted(changed.items(), key=sort_key):
        code[k] = v.value

    # If there are only add-on-only fields, then just remove the file.
    if not (set(code.keys()) - set(ADDON_ONLY_FIELDS)):
        code.clear()

    if code and not os.path.isdir('debian/upstream'):
        os.makedirs('debian/upstream', exist_ok=True)


fields = [
    ('%s (from %s)' % (v.field, v.origin)) if v.origin else v.field
    for k, v in sorted(changed.items())
    ]

print('Set upstream metadata fields: %s.' % ', '.join(sorted(fields)))
print('Certainty: %s' % achieved_certainty)
if fixed_tag:
    print('Fixed-Lintian-Tags: upstream-metadata-file-is-missing')
