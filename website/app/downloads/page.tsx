import type { Metadata } from "next";
import { Footer } from "../Footer";
import { Header } from "../Header";
import { DownloadLinks, type Platform } from "./DownloadLinks";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "Amalith Downloads — Coming Soon",
  description: "Amalith for macOS, Windows, and Linux is coming soon.",
};

const platforms: Platform[] = [
  {
    name: "macOS",
    icon: `${basePath}/brand/platform/mac.svg`,
  },
  {
    name: "Windows",
    icon: `${basePath}/brand/platform/win.svg`,
  },
  {
    name: "Linux",
    icon: `${basePath}/brand/platform/linux.svg`,
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
            <p className="downloads-disclaimer">
              Amalith is currently in alpha and isn&rsquo;t ready for prime time yet. If you&rsquo;d like to contribute, visit the <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">Amalith GitHub repository</a>.
            </p>
          </div>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
