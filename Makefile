VERSION=$(shell dpkg-parsechangelog | grep Version: | cut -d " " -f 2)

export RUST_LOG

default: check

.PHONY: build

check:: testsuite tag-status

.PHONY: testsuite unsupported

tag-status::
	$(MAKE) -C lintian-brush tag-status

testsuite::
	cargo test --workspace

README.md::
	cargo run -p lintian-brush --bin tag-status -- --update-readme

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

update-renamed-tags:
	$(MAKE) -C lintian-brush update-renamed-tags

update: update-spdx update-readme update-renamed-tags update-deps

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
