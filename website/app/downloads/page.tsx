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
    label: "Download for macOS",
    matchExt: ".dmg",
  },
  {
    name: "Windows",
    icon: `${basePath}/brand/platform/win.svg`,
    label: "Download for Windows",
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

      <main id="top" className="downloads-page">
        <section className="downloads-hero" aria-labelledby="downloads-title">
          <div className="downloads-hero__inner">
            <p className="kicker"><span /> Finally</p>
            <h1 id="downloads-title">
              <span>The design tool that lets</span>
              <span>creatives <em>create.</em></span>
            </h1>
            <DownloadLinks platforms={platforms} />
          </div>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
