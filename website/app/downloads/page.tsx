import type { Metadata } from "next";
import { Header } from "../Header";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "Download Amalith — Coming Soon",
  description: "Amalith downloads for macOS, Windows 10/11, and Linux are coming soon.",
};

const platforms = [
  {
    number: "01",
    name: "macOS",
    detail: "Native desktop app",
    note: "Built to feel at home on Mac, with a fast native canvas and familiar creative workflows.",
    icon: "mac",
  },
  {
    number: "02",
    name: "Windows 10/11",
    detail: "64-bit desktop app",
    note: "A self-contained Windows build designed to get you from download to canvas quickly.",
    icon: "windows",
  },
  {
    number: "03",
    name: "Linux",
    detail: "Multiple package formats",
    note: "AppImage, Debian, RPM, Arch Linux, and Flatpak options are planned for launch.",
    icon: "linux",
  },
] as const;

function PlatformIcon({ platform }: { platform: (typeof platforms)[number]["icon"] }) {
  if (platform === "windows") {
    return (
      <div className="platform-icon platform-icon--windows" aria-hidden="true">
        <span /><span /><span /><span />
      </div>
    );
  }

  return (
    <div className={`platform-icon platform-icon--${platform}`} aria-hidden="true">
      <span>{platform === "mac" ? "⌘" : ">_"}</span>
    </div>
  );
}

export default function Downloads() {
  return (
    <>
      <Header basePath={basePath} />

      <main id="top" className="downloads-page">
        <section className="downloads-hero section-shell" aria-labelledby="downloads-title">
          <p className="kicker"><span /> Download Amalith</p>
          <div className="downloads-hero__copy">
            <h1 id="downloads-title">Choose your canvas.<br /><em>Coming soon.</em></h1>
            <p>Native builds for the three desktops we call home. Amalith is still taking shape in public; downloads will appear here when they’re ready.</p>
          </div>
        </section>

        <section className="download-grid section-shell" aria-label="Available platforms">
          {platforms.map((platform) => (
            <article className="download-card" key={platform.name}>
              <div className="download-card__topline">
                <p>{platform.number} / Desktop</p>
                <span>In development</span>
              </div>
              <PlatformIcon platform={platform.icon} />
              <div className="download-card__copy">
                <p>{platform.detail}</p>
                <h2>{platform.name}</h2>
                <p>{platform.note}</p>
              </div>
              <button className="download-button" type="button" disabled>
                <span>Download</span>
                <strong>Coming soon</strong>
              </button>
            </article>
          ))}
        </section>

        <section className="downloads-note section-shell">
          <p className="section-number">Built in public</p>
          <div>
            <h2>Want to watch it come together?</h2>
            <p>Follow development, report issues, or contribute directly on GitHub.</p>
            <a className="text-link" href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">
              Explore the repository <span aria-hidden="true">→</span>
            </a>
          </div>
        </section>
      </main>

      <footer className="downloads-footer">
        <a className="footer-brand" href={`${basePath}/`} aria-label="Amalith home">
          <img src={`${basePath}/brand/amalith-a.svg`} alt="" />
        </a>
        <p>Free · open source · cross-platform</p>
        <a href={`${basePath}/`}>Back to Amalith <span aria-hidden="true">→</span></a>
      </footer>
    </>
  );
}
