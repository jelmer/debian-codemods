VERSION=$(shell dpkg-parsechangelog | grep Version: | cut -d " " -f 2)

export RUST_LOG

default: check

.PHONY: build

check:: testsuite tag-status check-manpages

.PHONY: testsuite unsupported check-manpages

check-manpages:
	for f in debian/*.1 debian/*.5; do man --warnings -l "$$f" > /dev/null || exit 1; done

tag-status::
	$(MAKE) -C lintian-brush tag-status

testsuite::
	cargo test --workspace

lintian-tags:
	lintian-explain-tags --list-tags > lintian-tags

.PHONY: lintian-tags lintian-brush-tags

unsupported: lintian-tags lintian-brush-tags
	awk 'NR==FNR{a[$$0]=1;next}!a[$$0]' lintian-brush-tags lintian-tags

update-spdx:
	python3 download-license-data.py > spdx.json
	brz diff spdx.json || brz commit -m "Update SPDX license data" spdx.json

update-renamed-tags:
	$(MAKE) -C lintian-brush update-renamed-tags

update: update-spdx update-lintian-brush-readme update-renamed-tags update-deps

update-lintian-brush-readme:
	$(MAKE) -C lintian-brush README.md

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

update-deps:
	brz diff debian/control || { echo "Pending changes to debian/control"; exit 1; }
	update-rust-deps
	brz diff debian/control || brz commit -m "Update Rust dependencies" debian/control
