import type { Metadata } from "next";
import { Footer } from "../Footer";
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
            <p className="kicker"><span /> Finally</p>
            <h1 id="downloads-title">
              <span>The design tool that lets</span>
              <span>creatives <em>create.</em></span>
            </h1>

            <div className="download-actions" aria-label="Desktop downloads coming soon">
              {platforms.map((platform) => (
                <button className="platform-download" type="button" key={platform.name}>
                  <img src={platform.icon} alt="" aria-hidden="true" />
                  <span>Coming soon for {platform.name}</span>
                </button>
              ))}
            </div>
          </div>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
