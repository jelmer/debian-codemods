VERSION=$(shell dpkg-parsechangelog | grep Version: | cut -d " " -f 2)

check:: style testsuite

.PHONY: style testsuite unsupported

style::
	flake8

testsuite::
	python3 setup.py test

README.md::
	PYTHONPATH=. ./buildtools/update-readme.py

unsupported:
	lintian-info --list-tags > lintian-tags
	PYTHONPATH=. python3 -m lintian_brush --list-tags 2> lintian-brush-tags
	awk 'NR==FNR{a[$$0]=1;next}!a[$$0]' lintian-brush-tags lintian-tags

update-readme:
	brz diff README.md
	$(MAKE) README.md
	brz diff README.md || brz commit -m "Update list of fixers in README.md." README.md

release: check update-readme
	./setup.py sdist
	twine upload --sign dist/lintian-brush-$(VERSION).tar.gz

update-spdx:
	python3 download-license-data.py > spdx.json
	brz diff spdx.json || brz commit -m "Update SPDX license data." spdx.json

update: update-spdx update-readme
