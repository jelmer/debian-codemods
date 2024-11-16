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
	$(MAKE) -C lintian-brush tag-status

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

update-spdx:
	python3 download-license-data.py > spdx.json
	brz diff spdx.json || brz commit -m "Update SPDX license data" spdx.json

update-key-package-versions:
	python3 analyzer/key-package-versions.py analyzer/key-package-versions.json
	brz diff analyzer/key-package-versions.json || brz commit -m "Update key package versions" analyzer/key-package-versions.json

update-renamed-tags:
	$(MAKE) -C lintian-brush update-renamed-tags

update: update-spdx update-readme update-renamed-tags update-key-package-versions

next:
	$(MAKE) -C lintian-brush next

docker:
	buildah build -t ghcr.io/jelmer/lintian-brush:latest Dockerfile.lintian-brush .
	buildah push ghcr.io/jelmer/lintian-brush:latest
	buildah build -t ghcr.io/jelmer/deb-scrub-obsolete:latest Dockerfile.deb-scrub-obsolete .
	buildah push ghcr.io/jelmer/deb-scrub-obsolete:latest
	buildah build -t ghcr.io/jelmer/debianize:latest Dockerfile.debianize .
	buildah push ghcr.io/jelmer/debianize:latest
	buildah build -t ghcr.io/jelmer/debianize:latest Dockerfile.debianize .
	buildah push ghcr.io/jelmer/debianize:latest

