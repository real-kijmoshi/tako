# Tako landing page

Static HTML, CSS, JavaScript, and SVG. No build step, dependencies, analytics, or external fonts.

For Cloudflare Pages, upload this folder with Direct Upload. For Git integration,
use the repository root, leave the build command empty, and set the output directory
to `landing`. Keep `_headers` alongside `index.html` for the security headers.

For local preview, run `python3 -m http.server 8080 --directory landing` from the
repository root and open `http://localhost:8080`.

The fallback demo is illustrative and sends no model requests. The install command
and GitHub links point to `real-kijmoshi/tako`; no domain configuration is required.
