#!/usr/bin/python3

import json
import sys
from urllib.request import urlopen

BASE_URL = (
    'https://raw.githubusercontent.com/spdx/license-list-data/master/json/')
LICENSES_URL = BASE_URL + 'licenses.json'
EXCEPTIONS_URL = BASE_URL + 'exceptions.json'

licenses = json.loads(urlopen(LICENSES_URL).read())
license_ids = []
for license in licenses['licenses']:
    license_ids.append(license['licenseId'])

exceptions = json.loads(urlopen(EXCEPTIONS_URL).read())
exception_ids = []
for exception in exceptions['exceptions']:
    exception_ids.append(exception['licenseExceptionId'])
result = {'license_ids': license_ids, 'exception_ids': exception_ids}
json.dump(result, sys.stdout, sort_keys=True, indent=4)
