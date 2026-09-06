import type { Metadata } from "next";
import { ArrowUpRight } from "../ArrowUpRight";
import { Footer } from "../Footer";
import { Header } from "../Header";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "The Cost of Free — Why Amalith",
  description: "Why Amalith is building an open, professional alternative for designers.",
};

export default function WhyAmalith() {
  return (
    <>
      <Header basePath={basePath} />

      <main tabIndex={-1} id="top" className="why-page why-cost-page" aria-label="Why Amalith">
        <section className="why-cost-hero" aria-labelledby="why-cost-title">
          <div className="why-cost-hero__inner">
            <p className="kicker">Why Amalith</p>
            <h1 id="why-cost-title">The Cost of <em>Free.</em></h1>
            <p className="why-cost-hero__lede">When the price is zero, the bill moves somewhere else.</p>
          </div>
          <p className="why-cost-hero__note">01 / The premise</p>
        </section>

        <section className="why-cost-intro section-shell">
          <p className="section-number">02 / The trade</p>
          <div>
            <h2>Free to download can still be expensive to <em>use.</em></h2>
            <p>Creative tools make money in ways that are easy to miss: subscriptions, locked formats, cloud dependence, and the quiet cost of relearning a workflow when the terms change.</p>
          </div>
        </section>

        <section className="why-cost-grid section-shell" aria-label="The costs of free software">
          <article>
            <p className="section-number">03 / Your files</p>
            <h2>Work should not be a <em>hostage.</em></h2>
            <p>When the format, storage, or export path is closed, your own work becomes leverage. Amalith keeps the document local and the format open.</p>
          </article>
          <article>
            <p className="section-number">04 / Your time</p>
            <h2>Subscriptions charge rent on <em>momentum.</em></h2>
            <p>A professional tool should reward fluency—not turn a familiar workflow into a recurring negotiation.</p>
          </article>
          <article>
            <p className="section-number">05 / Your options</p>
            <h2>Lock-in is a product <em>decision.</em></h2>
            <p>Investor pressure can favor growth, capture, and retention over durable ownership. Designers deserve another set of incentives.</p>
          </article>
        </section>

        <section className="why-cost-answer section-shell">
          <div>
            <p className="section-number">06 / The answer</p>
            <h2>Make the tool <em>yours.</em></h2>
          </div>
          <div className="why-cost-answer__copy">
            <p>Amalith is free to use today, with no payment or mandatory account required. We plan to charge in the future, but have not announced when, how much, or which offerings will be paid.</p>
            <ul>
              <li>Open documents you can keep and move</li>
              <li>A shared command engine, with broader automation integrations planned</li>
              <li>Local documents and familiar vector editing workflows</li>
            </ul>
            <a className="text-link" href={`${basePath}/downloads/`}>See the download plan <ArrowUpRight /></a>
          </div>
        </section>

        <section className="why-cost-allies section-shell" aria-labelledby="why-cost-allies-title">
          <div className="why-cost-allies__heading">
            <p className="section-number">07 / Good company</p>
            <h2 id="why-cost-allies-title">More open tools make a <em>stronger</em> space.</h2>
          </div>
          <div className="why-cost-allies__copy">
            <p>Amalith is not the only project pushing creative software forward. Inkscape and Graphite Editor are doing important, ambitious work—and both deserve your attention and support.</p>
            <div className="why-cost-allies__links">
              <a href="https://inkscape.org/" target="_blank" rel="noreferrer">
                <span><strong>Inkscape</strong><small>Free and open-source vector graphics</small></span>
                <ArrowUpRight />
              </a>
              <a href="https://graphite.art/" target="_blank" rel="noreferrer">
                <span><strong>Graphite Editor</strong><small>Open-source 2D and procedural graphics</small></span>
                <ArrowUpRight />
              </a>
            </div>
          </div>
        </section>


      </main>

      <Footer basePath={basePath} />
    </>
  );
}
