import type { Metadata } from "next";
import { Header } from "../Header";
import { Footer } from "../Footer";
const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";
export const metadata: Metadata = { title: 'Contact & accessibility — Amalith' };
export default function Page() {
  return <><Header basePath={basePath} /><main id="top" tabIndex={-1} className="policy-page"><h1>Contact &amp; accessibility</h1><p>Amalith is an independently developed vector editor. The project’s public repository is <a href="https://github.com/tonykastaneda/Amalith">tonykastaneda/Amalith on GitHub</a>.</p>
<h2>Project support</h2><p>Report bugs or ask project questions through <a href="https://github.com/tonykastaneda/Amalith/issues">GitHub issues</a>. GitHub is an external service and may require an account to participate. Issues are public: do not include confidential files or sensitive personal information.</p>
<h2>Accessibility feedback</h2><p>We are working to make this website usable with keyboards, readable contrast, descriptive links, and reduced-motion preferences. If you encounter a barrier, you can report it through the project’s issue tracker. Include the page address, what you were trying to do, and your browser or assistive technology if you are comfortable sharing it.</p>
<h2>Privacy</h2><p>Read our <a href="../privacy/">Privacy &amp; cookies</a> notice for information about this website and its hosting provider. Do not submit private privacy requests through public issues.</p></main><Footer basePath={basePath} /></>;
}
