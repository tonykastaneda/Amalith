# Amalith website

The marketing site for [Amalith](../README.md) — a static Next.js export served
from GitHub Pages at <https://www.amalith.app/>.

## Develop

```bash
npm install
npm run dev:pages      # serves at http://localhost:3000/
```

## Build

```bash
npm run build:pages    # static export to ./out for www.amalith.app
```

## Deploy

Pushing to `main` with changes under `website/**` triggers
`.github/workflows/pages.yml`, which runs `npm ci && npm run build:pages` and
publishes `out/` to GitHub Pages.

## Layout

- `app/` — the page (`page.tsx`), shell (`layout.tsx`), header (`Header.tsx`),
  styles (`globals.css`, Tailwind v4)
- `public/brand/` — logo assets, `public/og.png` — social card
- `next.config.ts` — `output: "export"` + `basePath` from `NEXT_PUBLIC_BASE_PATH`
