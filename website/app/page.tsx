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
    body: "The mouse, keyboard, plugins, scripts, CLI, and agents all speak the same operation language. Every change remains undoable and every workflow stays consistent.",
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

      <main id="top">
        <section className="hero" aria-labelledby="hero-title">
          <p className="kicker"><span /> Free · open source · cross-platform</p>
          <h1 id="hero-title">Design freely.<br /><em>Keep the power.</em></h1>
          <div className="hero__bottom">
            <p>A professional vector editor built for familiar workflows—and a future where every action is equally available to people, scripts, plugins, and agents.</p>
            <a className="circle-link" href="#why" aria-label="Explore Amalith"><span aria-hidden="true">↓</span></a>
          </div>
        </section>

        <section className="hero-media section-shell" aria-label="Amalith product preview">
          <Placeholder label="Amalith interface preview placeholder" />
        </section>

        <section className="manifesto section-shell" id="why">
          <p className="section-number">01 / Why Amalith</p>
          <div>
            <h2>The vector editor that launched yesterday with <em>20 years of tutorials.</em></h2>
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
          <div className="principles__grid">
            <p>No subscription</p><p>No mandatory account</p><p>No proprietary cloud</p>
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
            <p>Pen drawing, object selection, fill and stroke editing, export UI, and the full CLI are still ahead. Follow the repository to watch—and help—the editor grow.</p>
            <a className="text-link" href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">Explore on GitHub <span aria-hidden="true">→</span></a>
          </div>
        </section>

        <section className="cta">
          <div className="cta__art" aria-hidden="true">
            <img src={`${basePath}/brand/amalith-mark.svg`} alt="" className="cta__mark" />
          </div>
          <p><span>The Open source vector design,</span><br /><span>suite without the compromise.</span></p>
          <a href={`${basePath}/downloads/`}>Get Amalith <ArrowUpRight /></a>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
