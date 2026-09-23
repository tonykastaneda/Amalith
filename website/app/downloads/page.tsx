import type { Metadata } from "next";
import { Footer } from "../Footer";
import { Header } from "../Header";
import { DownloadLinks, type Platform } from "./DownloadLinks";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "Download Amalith — Coming Soon",
  description: "Amalith downloads for macOS, Windows 10/11, and Linux are coming soon.",
};

const platforms: Platform[] = [
  {
    name: "macOS",
    icon: `${basePath}/brand/platform/mac.svg`,
    label: "Download .dmg",
    matchExt: ".dmg",
  },
  {
    name: "Windows",
    icon: `${basePath}/brand/platform/win.svg`,
    label: "Download .zip",
    matchExt: ".zip",
  },
  {
    name: "Linux",
    icon: `${basePath}/brand/platform/linux.svg`,
    label: "View releases",
    // Never matches, so DownloadLinks falls back to the release page —
    // Linux ships four package formats (tar.gz/deb/rpm/AppImage), so
    // sending users to pick one there beats guessing for them.
    matchExt: null,
  },
];

export default function Downloads() {
  return (
    <>
      <Header basePath={basePath} />

      <main tabIndex={-1} id="top" className="marketing-page downloads-page">
        <section className="download-stage" aria-labelledby="downloads-title">
          <p className="kicker">Finally</p>
          <h1 id="downloads-title">The design tool that lets<br />creatives <em>create.</em></h1>
          <p className="download-stage__intro">Your ideas. Your files. Your workspace.<br />Free today. No payment or card required.</p>
          <DownloadLinks platforms={platforms} />
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
