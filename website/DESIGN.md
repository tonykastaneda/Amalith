# Amalith website design system

Reference: [Linear analysis from VoltAgent/awesome-design-md](https://github.com/voltagent/awesome-design-md/blob/main/design-md/linear.app/DESIGN.md), adapted to Amalith's own identity. Use its consistent spacing, typographic hierarchy, dark bordered surfaces and restrained actions; do not copy its brand colors or proprietary fonts.

## Protected surfaces

The Docs page and navigation are intentionally separate designs. Do not change `app/docs/page.tsx`, `Header.tsx`, their CSS, global base styles or logo assets as part of marketing restyling. The shared Footer also stays unchanged so Docs remains visually identical. New styles live in `app/marketing.css`, and every selector must be rooted in `.marketing-page`. Never set marketing tokens on `:root` or `body`.

## Identity

Retain yellow `#ffc619`, warm white `#f5f2ea`, ink `#11110f`, canvas `#050505`, existing neutral surfaces and all logo assets. Use yellow for primary actions and serif emphasis. No additional accent palette, external fonts, glow effects or decorative gradients. Keep Arial/Helvetica body and Georgia emphasis.

## Layout and typography

- Shared MarketingHero across Home, Why, Downloads, Privacy, Terms and Contact.
- Fluid page width with responsive gutters 24–72px; no maximum-width outer container. Keep readable paragraph measures.
- Section spacing 64–112px; spacing steps 8, 12, 16, 24, 32, 48, 64px.
- Display 48–96px, weight 600, line height 1.04; section headings 34–56px, line height 1.08.
- Body 18px / 1.6, reading pages 17px / 1.75; muted text uses `#b9b7ae`.
- 12px uppercase eyebrows, consistent spacing before titles and paragraphs.
- No marketing cards, rounded section containers, filled content panels, icon tiles, or bordered status badges anywhere. Use open sections, typography and thin rules. Product previews may depict application UI but get no outer card frame. Buttons may retain 8px corners.
- Actions at least 48px tall; yellow primary, bordered dark secondary. Availability is static status text, never a fake download button.
- PolicyLayout provides one shared project-information navigation and reading column.
- Narrow layouts stack columns; avoid fixed minimum widths and horizontal overflow.

## Content and accessibility

Preserve policy wording and the distinction between free today and uncertain future pricing. Preserve image alt text, visible focus, reduced-motion behavior and skip navigation. Product placeholders stay explicitly labeled; do not present them as real screenshots. Do not add unsupported claims, reviews, tracking or payment flows.
