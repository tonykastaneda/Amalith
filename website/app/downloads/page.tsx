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

      <main tabIndex={-1} id="top" className="downloads-page">
        <section className="downloads-hero" aria-labelledby="downloads-title">
          <div className="downloads-hero__inner">
            <p className="kicker">Finally</p>
            <h1 id="downloads-title">
              <span>The design tool that lets</span>
              <span>creatives <em>create.</em></span>
            </h1>

            <p className="download-note">Amalith is free today. No payment or card is required. We plan to charge in the future, but no date or pricing has been announced.</p>
            <p className="download-note">Packaged downloads are coming soon. You can <a href="https://github.com/tonykastaneda/Amalith#build-and-run">build from source</a> now.</p>

            <div className="download-actions" aria-label="Desktop downloads coming soon">
              {platforms.map((platform) => (
                <span className="platform-download platform-download--unavailable" key={platform.name}>
                  <img src={platform.icon} alt="" aria-hidden="true" />
                  <span>Coming soon for {platform.name}</span>
                </span>
              ))}
            </div>
          </div>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
