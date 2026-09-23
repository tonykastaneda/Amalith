import { Header } from "./Header";
import { ArrowUpRight } from "./ArrowUpRight";
import { Footer } from "./Footer";
import "./home.css";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

/** Replace these illustrative previews with product recordings when available. */
function Recording({ subject, variant = "canvas" }: { subject: string; variant?: "canvas" | "tools" | "commands" }) {
  return (
    <figure className={`home-recording home-recording--${variant}`}>
      <div className="home-recording__chrome" aria-hidden="true">
        <span className="home-recording__dots"><i /><i /><i /></span>
        <span>{subject}</span><span>Amalith</span>
      </div>
      <div className="home-recording__canvas">
        <div className="home-recording__art" aria-hidden="true">
          <div className="home-sheet home-sheet--one"><span>Room<br />to create.</span><i /></div>
          <div className="home-sheet home-sheet--two"><span>Aa</span><small>Type. Shape. Explore.</small></div>
          <div className="home-sheet home-sheet--three"><i /><i /><i /></div>
        </div>
        <div className="home-recording__notice">
          <span className="home-recording__symbol" aria-hidden="true">↗</span>
          <span>{subject}<small>Recording coming soon</small></span>
        </div>
      </div>
      <figcaption><span>Amalith in motion</span><span>Illustrative placeholder · {subject}</span></figcaption>
    </figure>
  );
}

export default function Home() {
  return (
    <>
      <Header basePath={basePath} />
      <main id="top" tabIndex={-1} className="home-page">
        <div className="home-frame">
          <section className="home-hero" aria-labelledby="home-title">
            <h1 id="home-title">Design freely.<br /><span>A familiar canvas.<br />A whole new possibility.</span></h1>
            <p>Amalith brings your tools, artboards, and ideas into one open-source design space. Built for the way you think. Yours to make your own.</p>
            <div className="home-actions">
              <a className="home-button" href={`${basePath}/downloads/`}>Get Amalith <ArrowUpRight /></a>
              <a className="home-link" href="#features">Explore the canvas <span aria-hidden="true">↓</span></a>
            </div>
          </section>
          <Recording subject="The Amalith canvas" />
          <div className="home-platforms" aria-label="Project at a glance">
            <span>Open source. Built in public.</span>
            <span>macOS / Windows / Linux</span>
            <span>Early development</span>
          </div>
          <section className="home-intro" id="features">
            <p className="home-eyebrow">Meet Amalith</p>
            <h2>One place for your ideas.<br /><span>From the first mark<br />to the final detail.</span></h2>
            <p>Space to explore. Tools that feel familiar. An open foundation you can shape around your own creative process.</p>
            <div className="home-disciplines" aria-label="Design disciplines"><span>Vector</span><span>Raster</span><span>Typography</span><span>Automation</span></div>
          </section>
          <section className="home-chapter" id="why">
            <div className="home-chapter__copy">
              <p className="home-eyebrow">Room to think</p>
              <h2>Your ideas don’t end<br />at the artboard.</h2>
              <p>Spread out on an infinite pasteboard. Keep references, experiments, and finished work together, with room for whatever comes next.</p>
            </div>
            <Recording subject="Exploring the infinite pasteboard" />
          </section>
          <section className="home-chapter">
            <div className="home-chapter__copy">
              <p className="home-eyebrow">Familiar by design</p>
              <h2>Less finding your tools.<br />More finding your flow.</h2>
              <p>Familiar panels, shortcuts, and ways of working. A growing set of vector, raster, and text tools, together on one canvas.</p>
            </div>
            <Recording subject="Tools, layers, and type" variant="tools" />
          </section>
          <section className="home-chapter">
            <div className="home-chapter__copy">
              <p className="home-eyebrow">One command engine</p>
              <h2>Make it by hand.<br />Make it your own.</h2>
              <p>The same command engine sits behind the interface, scripts, and agents. A foundation for automating the repetition and spending more time on the work you care about.</p>
            </div>
            <Recording subject="From canvas to commands" variant="commands" />
          </section>
          <section className="home-open" id="status">
            <div>
              <p className="home-eyebrow">Built in public</p>
              <h2>Early days.<br /><span>Wide-open possibilities.</span></h2>
            </div>
            <div>
              <p>Amalith is in active development and free to try. Features and platform support are still taking shape. Follow the repository for what works today and what’s next.</p>
              <a className="home-link" href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">Follow the project <ArrowUpRight /></a>
            </div>
          </section>
          <section className="home-closing">
            <p className="home-eyebrow">A space of your own</p>
            <h2>Bring your ideas.<br /><span>See where they take you.</span></h2>
            <a className="home-button" href={`${basePath}/downloads/`}>Get Amalith <ArrowUpRight /></a>
          </section>
        </div>
      </main>
      <Footer basePath={basePath} />
    </>
  );
}
