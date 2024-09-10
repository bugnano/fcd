# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2024-09-13

### Added

- Fzf-like file finder invoked with CTRL-P
- New keys for tagging and untagging: t, T, u, U
- The commands that required a function key, now can be invoked with a numeric
  key as well (for example for making a directory you can use either `F7` or `7`)

### Changed

- The quick viewer now is invoked with ALT-Q instead of CTRL-Q
- The `u` key no longer is an undo, but now it untags the selected entry

