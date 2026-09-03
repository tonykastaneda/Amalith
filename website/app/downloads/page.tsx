import type { Metadata } from "next";
import { Header } from "../Header";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "Download Amalith — Coming Soon",
  description: "Amalith downloads for macOS, Windows 10/11, and Linux are coming soon.",
};

const platforms = [
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
] as const;

export default function Downloads() {
  return (
    <>
      <Header basePath={basePath} />

      <main id="top" className="downloads-page">
        <section className="downloads-hero" aria-labelledby="downloads-title">
          <div className="downloads-hero__inner">
            <p className="kicker"><span /> Download Amalith</p>
            <h1 id="downloads-title">
              Finally, the design tool that lets<br />
              creatives <em>create.</em>
            </h1>

            <div className="download-actions" aria-label="Desktop downloads coming soon">
              {platforms.map((platform) => (
                <button className="platform-download" type="button" disabled key={platform.name}>
                  <img src={platform.icon} alt="" aria-hidden="true" />
                  <span>Coming soon for {platform.name}</span>
                </button>
              ))}
            </div>
          </div>
        </section>
      </main>

      <footer className="downloads-footer">
        <a className="footer-brand" href={`${basePath}/`} aria-label="Amalith home">
          <img src={`${basePath}/brand/amalith-a.svg`} alt="" />
        </a>
        <p>Free · open source · cross-platform</p>
        <a href={`${basePath}/docs/`}>Docs</a>
        <a href={`${basePath}/`}>Back to Amalith <span aria-hidden="true">→</span></a>
      </footer>
    </>
  );
}
