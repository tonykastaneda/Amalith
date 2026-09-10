import { MarketingHero } from "./MarketingHero";
import { Header } from "./Header";
import { ArrowUpRight } from "./ArrowUpRight";
import { Footer } from "./Footer";

const features = [
  {
    eyebrow: "Familiar by design",
    title: "Your instincts already know where to go.",
    body: "Amalith keeps the shortcuts, tools, artboards, and editing conventions professional vector designers expect. Spend your time making—not relearning.",
    tone: "light",
    label: "Product view placeholder",
  },
  {
    eyebrow: "One command engine",
    title: "Draw it. Script it. Agent it.",
    body: "The editor uses a shared command engine. Broader access for scripts, plugins, CLI tools, and agents is a development goal; those integrations are not all available today.",
    tone: "yellow",
    label: "Command engine diagram placeholder",
  },
  {
    eyebrow: "Infinite pasteboard",
    title: "Artboards are pages—not walls.",
    body: "Arrange artboards anywhere, keep objects between them, and build across an open document space without running into an arbitrary canvas edge.",
    tone: "dark",
    label: "Infinite canvas placeholder",
  },
];

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

function Placeholder({ label, variant = "window" }: { label: string; variant?: "window" | "canvas" }) {
  return (
    <div className={`placeholder placeholder--${variant}`} role="img" aria-label={label}>
      <div className="placeholder__bar"><span /><span /><span /></div>
      <div className="placeholder__workspace">
        <div className="placeholder__tools" />
        <div className="placeholder__stage">
          <div className="placeholder__artboard" />
          <div className="placeholder__artboard placeholder__artboard--small" />
        </div>
        <div className="placeholder__panel" />
      </div>
      <span className="placeholder__label">{label}</span>
    </div>
  );
}

export default function Home() {
  return (
    <>
      <Header basePath={basePath} />

      <main tabIndex={-1} id="top" className="marketing-page">
        <MarketingHero
          eyebrow="Free today · open source · cross-platform"
          title={<>Design freely.<br /><em>Keep the power.</em></>}
          actions={<><a className="marketing-button" href={`${basePath}/downloads/`}>Get Amalith <ArrowUpRight /></a><a className="marketing-button marketing-button--secondary" href="#features">Explore features <span aria-hidden="true">↓</span></a></>}
        >
          <p>A professional vector editor built for familiar workflows—and a future where every action is equally available to people, scripts, plugins, and agents.</p>
        </MarketingHero>

        <section className="hero-media section-shell" aria-label="Amalith product preview">
          <Placeholder label="Amalith interface preview placeholder" />
        </section>

        <section className="manifesto section-shell" id="why">
          <p className="section-number">01 / Why Amalith</p>
          <div>
            <h2>A vector editor built around <em>familiar workflows.</em></h2>
            <p>Amalith is being built so experienced Illustrator users can sit down and begin—without giving up openness, automation, or ownership of their work.</p>
          </div>
        </section>

        <section className="feature-stack" id="features" aria-label="Amalith features">
          {features.map((feature, index) => (
            <article className={`feature feature--${feature.tone}`} key={feature.title}>
              <div className="feature__copy">
                <p className="section-number">0{index + 2} / {feature.eyebrow}</p>
                <h2>{feature.title}</h2>
                <p>{feature.body}</p>
              </div>
              <Placeholder label={feature.label} variant={index === 2 ? "canvas" : "window"} />
            </article>
          ))}
        </section>

        <section className="principles section-shell">
          <p className="section-number">05 / Built in public</p>
          <h2>Open to All.<br /><em>Yours to shape.</em></h2>
          <p className="section-intro">Amalith is free today. We plan to charge in the future, but no date or pricing has been announced.</p>
          <div className="principles__grid">
            <p>No payment required</p><p>No mandatory account</p><p>No proprietary cloud</p>
            <p>Open document format</p><p>macOS, Windows &amp; Linux</p><p>MIT or Apache 2.0</p>
          </div>
        </section>

        <section className="status section-shell" id="status">
          <div>
            <p className="section-number">Current status</p>
            <h2>Early, active, and taking shape.</h2>
          </div>
          <div className="status__copy">
            <p>Amalith is in early development. The native desktop app already has documents, multiple artboards, tabs, an infinite pasteboard, save/load, undoable commands, and core canvas navigation.</p>
            <p>Features and platform support are evolving. Check the repository for current implementation details and known limitations; previews on this site are illustrative placeholders.</p>
            <a className="text-link" href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">Explore on GitHub <span aria-hidden="true">→</span></a>
          </div>
        </section>

        <section className="cta">
          <div className="cta__art" aria-hidden="true">
            <img src={`${basePath}/brand/amalith-mark.svg`} alt="" className="cta__mark" />
          </div>
          <h2>Open-source design.<br /><em>Room to create.</em></h2>
          <a className="marketing-button" href={`${basePath}/downloads/`}>Get Amalith <ArrowUpRight /></a>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
