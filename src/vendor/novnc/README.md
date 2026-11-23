# noVNC (vendored)

The VNC client the screen window draws a lab machine's screen with (`src/app/screen/`).

- **Version:** noVNC 1.6.0, from <https://github.com/novnc/noVNC/archive/refs/tags/v1.6.0.tar.gz>
  (SHA-256 `5066103959ef4e9b10f37e5a148627360dd8414e4cf8a7db92bdbd022e728aaa`).
- **What is here:** its `core/` library and the `vendor/pako/` it imports, as released, unchanged:
  ES modules, which the app bundles (npm's `@novnc/novnc` ships a CommonJS build with a top-level
  `await` no bundler takes).
- **Licence:** the core library is under the Mozilla Public License 2.0 (`LICENSE.MPL-2.0`;
  `LICENSE.txt` says which files are under which licence), pako under the MIT licence
  (`vendor/pako/LICENSE`). The files are kept as they are and their source is the release
  above, as MPL-2.0 asks of a distribution in executable form.
- **Updating:** replace `core/` and `vendor/pako/` with a newer release's, then update this file,
  and `NOTICE` if its licence changed.
