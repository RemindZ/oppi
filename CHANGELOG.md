# Changelog

## 0.2.1 - 2026-04-29

### Added

- Added `/exit` as OPPi's graceful shutdown command so session shutdown cleanup, Hoppi exit recaps, and exit sync can run before the process closes.
- Added `suggest_next_message`, a high-confidence ghost reply/prediction tool that can show a grey next-message suggestion in the input box.

### Changed

- Updated the footer hint line while a ghost suggestion is visible: `Enter` sends it, `→` accepts it into the editor, and typing replaces it.
- Updated docs and prompt catalog entries for `/exit` and `suggest_next_message`.

## 0.2.0 - 2026-04-29

### Added

- First public npm release of `@oppiai/cli` and `@oppiai/pi-package`.
- Stage 2 `oppi` CLI wrapper with OPPi package launch defaults, isolated agent dir, `doctor`, and memory helper commands.
