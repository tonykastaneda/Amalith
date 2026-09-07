import { ArrowUpRight } from "../ArrowUpRight";
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

      <main tabIndex={-1} id="top" className="marketing-page downloads-page">
        <section className="download-stage" aria-labelledby="downloads-title">
          <p className="kicker">Amalith / Coming soon</p>
          <h1 id="downloads-title">The design tool that lets<br />creatives <em>create.</em></h1>
          <p className="download-stage__intro">Your ideas. Your files. Your workspace.<br />Free today. No payment or card required.</p>
          <div className="platform-grid">
            {platforms.map((platform) => (
              <article className="platform-card" key={platform.name}>
                <div className="platform-card__icon"><img src={platform.icon} alt="" aria-hidden="true" /></div>
                <h3>{platform.name}</h3>
                <span className="availability-label">Coming soon</span>
              </article>
            ))}
          </div>
          <a className="text-link" href="https://github.com/tonykastaneda/Amalith#build-and-run">Build from source while we get ready <ArrowUpRight /></a>
          <p className="pricing-note">We plan to charge in the future, but no date or pricing has been announced. <a href={`${basePath}/terms/`}>Read about pricing and payments <ArrowUpRight /></a></p>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
