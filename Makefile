.PHONY: build check dev install

PREFIX ?= $(HOME)/.local

dev:
	npm run tauri dev

build:
	npm run tauri build -- --no-bundle

check:
	npm run build
	cargo test --manifest-path src-tauri/Cargo.toml
	cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

install: build
	install -Dm755 src-tauri/target/release/keyventory "$(DESTDIR)$(PREFIX)/bin/keyventory"
