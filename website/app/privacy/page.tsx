import type { Metadata } from "next";
import { Header } from "../Header";
import { Footer } from "../Footer";
const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";
export const metadata: Metadata = { title: 'Privacy & cookies — Amalith' };
export default function Page() {
  return <><Header basePath={basePath} /><main id="top" tabIndex={-1} className="policy-page"><h1>Privacy &amp; cookies</h1><p>Last updated: September 6, 2026. This notice covers the Amalith website, not all behavior of the desktop app or third-party services.</p>
<h2>Visiting this website</h2><p>The website does not have account registration, payment collection, newsletter forms, advertising pixels, or embedded third-party widgets. Its pages do not set application cookies or use local storage for tracking. Images and fonts are served locally with the site.</p>
<h2>Hosting and technical information</h2><p>This website is configured for GitHub Pages hosting. The hosting provider processes technical information such as your IP address and request details to deliver and protect the service. This means visiting a static website is not the same as sharing no data. See <a href="https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement">GitHub’s privacy statement</a> for its processing, retention, international transfers, and privacy rights.</p>
<h2>Cookies and tracking preferences</h2><p>The website itself does not use advertising or analytics cookies and does not track visitors across websites. Its behavior is the same with or without a Do Not Track browser signal. External websites may use their own cookies after you follow a link. If we introduce tracking or other optional storage, we will update this notice and provide choices or obtain consent where required before activating it.</p>
<h2>Information you choose to share</h2><p>If you participate in the project’s GitHub issues, your username and contributions may be public and are handled through GitHub. Share only what is needed to describe a problem. Do not post passwords, private documents, payment details, or sensitive personal information. Public issues are not a private channel for privacy requests.</p>
<h2>Questions and updates</h2><p>See our <a href="../contact/">contact page</a> for the project’s currently published contact route. Questions about information held by GitHub can be directed to GitHub using its privacy statement. Changes to this notice will be published here with an updated date.</p></main><Footer basePath={basePath} /></>;
}
