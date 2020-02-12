#!/usr/bin/python3

import json
import sys
from urllib.request import urlopen

BASE_URL = (
    'https://raw.githubusercontent.com/spdx/license-list-data/master/json/')
LICENSES_URL = BASE_URL + 'licenses.json'
EXCEPTIONS_URL = BASE_URL + 'exceptions.json'

licenses_summary = json.loads(urlopen(LICENSES_URL).read())
licenses = {}
license_ids = []
for license in licenses_summary['licenses']:
    license_ids.append(license['licenseId'])
    licenses[license['licenseId']] = {
        'name': license['name'],
        }

exceptions = json.loads(urlopen(EXCEPTIONS_URL).read())
exception_ids = []
for exception in exceptions['exceptions']:
    exception_ids.append(exception['licenseExceptionId'])
result = {'licenses': licenses, 'exception_ids': exception_ids}
json.dump(result, sys.stdout, sort_keys=True, indent=4)
