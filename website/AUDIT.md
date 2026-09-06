# Website checklist audit — September 6, 2026

Scope: website source and generated static export. This records the pre-deployment audit; deployment status is tracked by the GitHub Pages workflow.
This is a technical/content review, not legal clearance or a WCAG conformance certification.

| Checklist item | Finding / action |
| --- | --- |
| Colour contrast | Removed low-opacity navigation/footer hover states, made header background consistently dark, brightened low-contrast small text. Static palette check below; rendered browser check outstanding. |
| Image alt text | Logo has a name; decorative brand/platform images use empty alt; illustrative previews have accessible descriptions and visible placeholder labels. |
| Refund policy | Added Terms & payments: no purchases or subscriptions today, so nothing currently to refund/cancel; future purchase terms must precede payment collection. |
| Privacy policy | Added source-grounded website notice including hosting and external GitHub interactions. Operator identity, private contact, hosting configuration and actual retention still need owner verification before publication. |
| Accessibility | Added skip link, focusable main targets, visible keyboard outlines, sticky-header anchor offsets, reduced-motion header behavior, and narrow-screen layout fixes. Screen-reader and browser keyboard/reflow testing remains outstanding. |
| Fake reviews | No testimonials, ratings, or review widgets found. |
| Terms & conditions | Added current free status, uncertain future pricing, software/license distinction, development status, external links, and preservation of statutory rights. |
| Third-party embeds | No iframe, remote font, video, analytics widget, or third-party embed in application source. External links lead to other services on navigation. |
| Image copyright | Brand/platform SVGs, wordmark and social-card PNGs are local assets. Local storage does not prove rights. Owner must confirm source, ownership, permissions and trademark use for website/public/brand/** and website/public/og*.png. No license files found at repo root despite Cargo/README MIT OR Apache-2.0 declarations; license notice completion needs owner review. |
| Cookies policy | Combined Privacy & cookies page describes current application behavior and hosting caveat. |
| Tracking | No cookies/localStorage/sessionStorage/analytics/pixels or data-submission requests found in website application code. Hosting/CDN settings and live requests not verified. |
| Form consent | No forms, mailing list, sign-up, or checkout present. No consent checkbox needed for a nonexistent form. Reassess before adding collection. |
| Local laws | Footer currently identifies Bell, California, but operator jurisdiction is unconfirmed. CalOPPA, applicable state privacy rules, overseas audiences and future consumer/payment obligations require owner/legal review of actual operation. Free pricing alone does not settle privacy/accessibility obligations. |
| Button labels | Header now says Downloads; Features links target Features. Nonfunctional coming-soon download buttons became clearly labeled static availability notices. |
| Cookie consent | No application tracking to gate in current source; no ornamental consent banner added. Inspect hosting/CDN and live storage before concluding consent is unnecessary. Gate any future non-exempt tracking before activation. |
| Real business details | Added actual public repository/support route, no invented company/email/address. Public operator name, jurisdiction and private contact are outstanding. Public GitHub issues are explicitly unsuitable for private requests. |
| Necessary data only | No direct collection forms; public issue guidance asks for only necessary problem details and warns against sensitive uploads. Hosting/support retention and any off-repo processing remain unverified. |
| Keyboard-friendly forms | No forms. Native mobile menu remains keyboard-operable; keyboard focus and skip navigation improved in source. Full interactive verification remains outstanding. |
| Unsupported claims | Removed “launched yesterday / 20 years of tutorials,” unsourced investor/regulatory research section, stale missing-feature list, blanket finished automation claims, and permanent-sounding pricing promises. Future automation is labeled planned. |

## Before publishing these policy drafts

Confirm public operator identity and private contact route, country/state, hosting/CDN analytics and log practices, and asset rights. Complete the privacy notice with verified contact/retention/rights procedures appropriate to the operation. Do not use the public issue tracker for sensitive privacy requests. Verify production cookies/network behavior and perform desktop/mobile keyboard, zoom, screen-reader and contrast checks.

## Before charging

Decide the actual paid offer. Publish prices, purchase/refund/cancellation terms and applicable consumer disclosures; assess payment-provider data processing and consent requirements before taking money. The current pages do not promise permanent free access or authorize automatic future charges.

## Sources consulted

- W3C WCAG 2.2: https://www.w3.org/TR/wcag/
- California Attorney General, CalOPPA privacy notices: https://oag.ca.gov/sites/all/files/agweb/pdfs/cybersecurity/making_your_privacy_practices_public.pdf
- California Attorney General, CCPA applicability: https://oag.ca.gov/privacy/ccpa
- ICO cookies guidance: https://ico.org.uk/for-organisations/advice-for-small-organisations/privacy-notices-and-cookies/cookies-and-privacy-notices-in-detail/
- GitHub hosting/service privacy: https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement

## Verification limits

Live URL access failed in this environment (DNS resolution in shell; web fetch unavailable). Browser-control tools and Playwright are unavailable. No production tracking audit or rendered WCAG pass is claimed. Existing GitHub Pages workflow is preserved. The owner subsequently authorized committing and deploying these changes through the existing GitHub Pages workflow.

## Completed checks

- `npm run build:pages`: passed, all seven content routes exported.
- `npm run lint`: passed with four existing Next.js image-optimization warnings, no errors.
- Generated HTML: all seven pages have one main and one h1; all image tags have alt attributes; internal routes and fragment targets resolve.
- Six representative foreground/background pairs meet 4.5:1 (lowest checked: muted footer text, 5.75:1). This is a palette calculation, not a complete rendered contrast audit.
- `git diff --check -- website`: passed.
