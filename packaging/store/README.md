# Microsoft Store listing

Everything the Store shows for Cyber CTF lives here and goes out with each release
(`.github/workflows/msix.yml` → `.github/scripts/store-submit.ps1`, Partner Center submission API):

- `listings/<lang>.json`: title, descriptions, "What's new" (`releaseNotes`), features, search terms,
  links, screenshot captions. One file per language (`en-us`, `fr-fr`); add a language by adding a file.
- `images/<lang>/`: screenshots (`screenshot-N.png`, 16:9, ≥1366×768) and the 9:16 poster.
- `images/common/`: 1:1 box art (2160×2160) and the 300×300 app tile, shared by every language.
- `trailers/<lang>/`: `trailer.mp4` (1920×1080, H.264, <60 s) and its `thumbnail.png` (1920×1080).

The package itself (x64 + Arm64 `.msixbundle`) declares the app's languages and description from
`packaging/msix/` (manifest + `Strings/<lang>/Resources.resw`).

Update `releaseNotes` in each listing before tagging a release. Set the repository variable
`STORE_AUTO_SUBMIT=false` to have a release leave its Store submission as a draft to review in
Partner Center instead of sending it for certification.

Trailers need an audio track (the Store refuses a video without one, even a silent video): add a
silent stereo AAC track if the video has none.
