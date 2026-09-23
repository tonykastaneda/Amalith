import { MarketingHero } from "./MarketingHero";
import { Header } from "./Header";
import { ArrowUpRight } from "./ArrowUpRight";
import { Footer } from "./Footer";

const features = [
  {
    eyebrow: "Familiar by design",
    title: "Your instincts already know where to go.",
    body: "Same tools. Same shortcuts. Same muscle memory. You already know how to use Amalith.",
    tone: "light",
    label: "Product view placeholder",
  },
  {
    eyebrow: "One command engine",
    title: "Draw it. Script it. Agent it.",
    body: "One engine runs every action in the app. Your hands, your scripts, and your agents all use it the same way.",
    tone: "yellow",
    label: "Command engine diagram placeholder",
  },
  {
    eyebrow: "Infinite pasteboard",
    title: "Artboards are pages. Not walls.",
    body: "Your canvas doesn't end at an edge. Spread artboards out. Connect your work. Never hit a wall.",
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

function RecordingPlaceholder() {
  return (
    <div className="recording-placeholder" role="img" aria-label="Amalith product recording placeholder">
      <div className="recording-placeholder__grid" aria-hidden="true" />
      <div className="recording-placeholder__window">
        <span className="recording-placeholder__play" aria-hidden="true">▶</span>
        <div>
          <p className="recording-placeholder__eyebrow">Product recording</p>
          <p className="recording-placeholder__title">A closer look at Amalith is on the way.</p>
          <p className="recording-placeholder__note">We’re leaving this space ready for the first walkthrough.</p>
        </div>
      </div>
      <span className="recording-placeholder__label">Recording placeholder</span>
    </div>
  );
}

export default function Home() {
  return (
    <>
      <Header basePath={basePath} />

      <main tabIndex={-1} id="top" className="marketing-page">
        <MarketingHero
          eyebrow="The first real IDE for designers — not developers"
          title={<>Design freely.<br /><em>No subscription.</em></>}
          actions={<><a className="marketing-button" href={`${basePath}/downloads/`}>Get Amalith <ArrowUpRight /></a><a className="marketing-button marketing-button--secondary" href="#features">Explore features <span aria-hidden="true">↓</span></a></>}
        >
          <p>Built for artists. Not a pile of glued-together tools. Not a subscription.</p>
        </MarketingHero>

        <section className="recording-stage section-shell" aria-labelledby="recording-title">
          <div className="recording-stage__intro">
            <p className="section-number">01 / See it in motion</p>
            <h2 id="recording-title">A canvas that stays <em>out of your way.</em></h2>
            <p>When the walkthrough is ready, this is where we’ll show the real app: the tools, the pasteboard, and the little details that make Amalith feel familiar.</p>
          </div>
          <RecordingPlaceholder />
        </section>

        <section className="manifesto section-shell" id="why">
          <p className="section-number">02 / Why Amalith</p>
          <div>
            <h2>The design tool with <em>20 years of tutorials</em> that launched yesterday.</h2>
            <p>Every shortcut, panel, and keystroke you already know. Rebuilt from the ground up so it's yours to script, automate, and own. Not rent.</p>
          </div>
        </section>

        <section className="feature-stack" id="features" aria-label="Amalith features">
          <p className="section-number">03 / The full-circle workflow</p>
          <h2 className="feature-stack__intro">Create. Iterate. <em>Automate.</em></h2>
          <p className="feature-stack__lede">The design tool you already know, with superpowers. The full-circle, anti-slop workflow for humans who create integrated design systems.</p>
          {features.map((feature, index) => (
            <article className={`feature feature--${feature.tone}`} key={feature.title}>
              <div className="feature__copy">
                <p className="section-number">0{index + 4} / {feature.eyebrow}</p>
                <h2>{feature.title}</h2>
                <p>{feature.body}</p>
              </div>
              <Placeholder label={feature.label} variant={index === 2 ? "canvas" : "window"} />
            </article>
          ))}
        </section>

        <section className="principles section-shell">
          <p className="section-number">06 / Built in public</p>
          <h2>Open to All.<br /><em>Yours to shape.</em></h2>
          <p className="section-intro">Amalith is free, and we don&rsquo;t plan to ever charge for it.</p>
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
          <div className="cta__inner">
            <h2>Open-source design.<br /><em>Room to create.</em></h2>
            <a className="marketing-button" href={`${basePath}/downloads/`}>Get Amalith <ArrowUpRight /></a>
          </div>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
