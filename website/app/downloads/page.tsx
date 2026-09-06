import { MarketingHero } from "../MarketingHero";
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
        <MarketingHero
          eyebrow="Download Amalith"
          title={<>Your next idea.<br /><em>Your workspace.</em></>}
          actions={<a className="marketing-button" href="https://github.com/tonykastaneda/Amalith#build-and-run">Build from source <ArrowUpRight /></a>}
        >
          <p>Amalith is free today. No payment or card is required. Packaged downloads are coming soon; you can build from source now.</p>
        </MarketingHero>
        <section className="platform-section" aria-labelledby="platform-title">
          <p className="section-number">Desktop availability</p>
          <h2 id="platform-title">A place on <em>your desktop.</em></h2>
          <div className="platform-grid">
            {platforms.map((platform) => (
              <article className="platform-card" key={platform.name}>
                <div className="platform-card__icon"><img src={platform.icon} alt="" aria-hidden="true" /></div>
                <h3>{platform.name}</h3>
                <p>Packaged download</p>
                <span className="availability-label">Coming soon</span>
              </article>
            ))}
          </div>
          <p className="pricing-note">We plan to charge in the future, but no date or pricing has been announced. <a href={`${basePath}/terms/`}>Read about pricing and payments <ArrowUpRight /></a></p>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
