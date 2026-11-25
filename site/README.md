# GuitarCraft site

Download page for GuitarCraft: https://guitarcraft.netlify.app. Plain static files, no build step.

- `index.html`, `styles.css`, `app.js`: the page. Russian and English (the `T` table in `app.js`); the language
  follows the browser and the RU / EN buttons switch it.
- Installers are not hosted here: `app.js` asks the GitHub API for the latest release of this repository and links
  to its `.dmg`, `-setup.exe` and `.deb`. Publishing a release updates the page; the app updates itself from the
  same release (Tauri updater, signed `latest.json`).
- `netlify.toml`: Netlify settings (headers, `/download` redirect to the latest release, skip builds that do not
  touch `site/`). In Netlify set **Base directory** to `site`.
- `?intro=1` replays the opening animation, `?intro=0` skips it.

Preview locally: `python3 -m http.server` in this folder, then open http://localhost:8000.
