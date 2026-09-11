.PHONY: build check dev install

PREFIX ?= $(HOME)/.local
APP_ID := com.ey3g0r3.keyventory
BINDIR := $(PREFIX)/bin
DATADIR := $(PREFIX)/share

dev:
	npm run tauri dev

build:
	npm run tauri build -- --no-bundle

check:
	npm run build
	cargo test --manifest-path src-tauri/Cargo.toml
	cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

install: build
	install -Dm755 src-tauri/target/release/keyventory "$(DESTDIR)$(BINDIR)/keyventory"
	install -Dm644 packaging/$(APP_ID).desktop "$(DESTDIR)$(DATADIR)/applications/$(APP_ID).desktop"
	install -Dm644 src-tauri/icons/32x32.png "$(DESTDIR)$(DATADIR)/icons/hicolor/32x32/apps/$(APP_ID).png"
	install -Dm644 src-tauri/icons/128x128.png "$(DESTDIR)$(DATADIR)/icons/hicolor/128x128/apps/$(APP_ID).png"
	install -Dm644 src-tauri/icons/128x128@2x.png "$(DESTDIR)$(DATADIR)/icons/hicolor/256x256/apps/$(APP_ID).png"
