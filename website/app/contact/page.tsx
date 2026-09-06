import type { Metadata } from "next";
import { PolicyLayout } from "../PolicyLayout";

export const metadata: Metadata = { title: "Contact & accessibility — Amalith" };

export default function Page() {
  return (
    <PolicyLayout current="contact" title="Contact & accessibility" intro="Find project support and help us make Amalith more accessible.">
      <p>Amalith is an independently developed vector editor. The project’s public repository is <a href="https://github.com/tonykastaneda/Amalith">tonykastaneda/Amalith on GitHub</a>.</p>

      <h2>Project support</h2>
      <p>Report bugs or ask project questions through <a href="https://github.com/tonykastaneda/Amalith/issues">GitHub issues</a>. GitHub is an external service and may require an account to participate. Issues are public: do not include confidential files or sensitive personal information.</p>

      <h2>Accessibility feedback</h2>
      <p>We are working to make this website usable with keyboards, readable contrast, descriptive links, and reduced-motion preferences. If you encounter a barrier, you can report it through the project’s issue tracker. Include the page address, what you were trying to do, and your browser or assistive technology if you are comfortable sharing it.</p>

      <h2>Privacy</h2>
      <p>Read our <a href="../privacy/">Privacy &amp; cookies</a> notice for information about this website and its hosting provider. Do not submit private privacy requests through public issues.</p>
    </PolicyLayout>
  );
}
