VERSION=$(shell dpkg-parsechangelog | grep Version: | cut -d " " -f 2)

default: check

.PHONY: build

build:
	./setup.py build_ext -i

check:: testsuite tag-status ruff

.PHONY: ruff testsuite unsupported

ruff::
	ruff check py/ lintian-brush/fixers/

typing:: build
	mypy py/ lintian-brush/fixers/

tag-status::
	PYTHONPATH=$(shell pwd)/py python3 lintian-brush/tag-status.py --check

testsuite:: build
	PYTHONPATH=$(shell pwd)/py python3 -m unittest lintian_brush.tests.test_suite
	PYTHONPATH=$(shell pwd)/py cargo test 

README.md::
	PYTHONPATH=$(PWD)/py:$(PYTHONPATH) ./buildtools/update-readme.py

lintian-tags:
	lintian-explain-tags --list-tags > lintian-tags

.PHONY: lintian-tags lintian-brush-tags

unsupported: lintian-tags lintian-brush-tags
	awk 'NR==FNR{a[$$0]=1;next}!a[$$0]' lintian-brush-tags lintian-tags

update-readme:
	brz diff README.md
	$(MAKE) README.md
	brz diff README.md || brz commit -m "Update list of fixers in README.md" README.md

release: check update-readme
	./setup.py sdist
	twine upload --sign dist/lintian-brush-$(VERSION).tar.gz

update-spdx:
	python3 download-license-data.py > spdx.json
	brz diff spdx.json || brz commit -m "Update SPDX license data" spdx.json

update-key-package-versions:
	python3 key-package-versions.py
	brz diff key-package-versions.json || brz commit -m "Update key package versions" key-package-versions.json

update-renamed-tags:
	python3 lintian-brush/renamed-tags.py
	brz diff lintian-brush/renamed-tags.json || brz commit -m "Update renamed tags" lintian-brush/renamed-tags.json

update: update-spdx update-readme update-renamed-tags update-key-package-versions

next:
	python3 next.py

docker:
	buildah build -t ghcr.io/jelmer/lintian-brush:latest Dockerfile.lintian-brush .
	buildah push ghcr.io/jelmer/lintian-brush:latest
	buildah build -t ghcr.io/jelmer/deb-scrub-obsolete:latest Dockerfile.deb-scrub-obsolete .
	buildah push ghcr.io/jelmer/deb-scrub-obsolete:latest
	buildah build -t ghcr.io/jelmer/debianize:latest Dockerfile.debianize .
	buildah push ghcr.io/jelmer/debianize:latest
	buildah build -t ghcr.io/jelmer/debianize:latest Dockerfile.debianize .
	buildah push ghcr.io/jelmer/debianize:latest

