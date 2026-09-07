import type { Metadata } from "next";
import { MarketingHero } from "../MarketingHero";
import { Footer } from "../Footer";
import { Header } from "../Header";
import findings from "../../content/investor-findings.json";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";
export const metadata: Metadata = {
  title: "The Cost of Free — Why Amalith",
  description: "Research notes on legal and regulatory matters involving groups listed as Canva investors and Adobe institutional holders, with outcomes and attribution distinctions.",
};

export default function WhyAmalith() {
  return (
    <>
      <Header basePath={basePath} />
      <main tabIndex={-1} id="top" className="marketing-page research-page">
        <MarketingHero eyebrow="Why Amalith / Research" title={<>The Cost of <em>Free.</em></>}>
          <p>Look beyond the software. Examine the institutions behind it.</p>
        </MarketingHero>
        <div className="research-content">
          <section className="research-intro" aria-labelledby="research-title">
            <h2 id="research-title">Canva and Adobe.<br /><em>The investor record.</em></h2>
            <div>
              <p>This page presents the project’s research notes on legal and regulatory matters involving groups listed in the original write-up as Canva investors or Adobe institutional holders.</p>
              <p>These are historical matters involving named entities, individuals, predecessors, or portfolio companies. They do not establish misconduct by Canva or Adobe, or show that an investor directed either company’s product or pricing decisions.</p>
              <p>The groupings come from the original research, not a verified current ownership register. Institutional holdings change, and an asset manager may hold shares on behalf of clients. Sources below address the individual matters; they do not independently establish every ownership relationship.</p>
              <a className="text-link" href="https://github.com/tonykastaneda/Amalith/blob/main/website/content/canva-adobe-investor-findings.md">Read the original Markdown <span aria-hidden="true">↗</span></a>
            </div>
          </section>
          <nav className="research-index" aria-label="Research sections">
            <a href="#canva-investors">Canva investors <span aria-hidden="true">↓</span></a>
            <a href="#adobe-holders">Adobe holders <span aria-hidden="true">↓</span></a>
            <a href="#research-notes">How to read this research <span aria-hidden="true">↓</span></a>
          </nav>
          {findings.map((group, groupIndex) => (
            <section key={group.title} id={groupIndex === 0 ? "canva-investors" : "adobe-holders"} className="research-group" aria-labelledby={`group-${groupIndex}`}>
              <p className="section-number">0{groupIndex + 1} / Research record</p>
              <h2 id={`group-${groupIndex}`}>{group.title}</h2>
              {group.rows.map((row, rowIndex) => (
                <article className="research-entry" key={`${row.name}-${rowIndex}`}>
                  <h3>{row.name}</h3>
                  <div className="research-entry__detail">
                    <p>{row.matter}</p>
                    <dl>
                      <div><dt>Classification</dt><dd>{row.classification}</dd></div>
                      <div><dt>Outcome / status in the notes</dt><dd>{row.outcome}</dd></div>
                    </dl>
                    {row.source ? <a href={row.source}>Read the source <span aria-hidden="true">↗</span></a> : <p className="research-source-note">Research remains open. This describes the original research pass, not a finding of wrongdoing or a claim that no matter exists.</p>}
                    {row.sourceNote && <p className="research-source-note">{row.sourceNote}</p>}
                  </div>
                </article>
              ))}
            </section>
          ))}
          <section id="research-notes" className="research-notes" aria-labelledby="notes-title">
            <p className="section-number">03 / Method and attribution</p>
            <h2 id="notes-title">Read the <em>distinctions.</em></h2>
            <ul>
              <li><strong>Allegation</strong> means a claim was made; it does not mean the claim was proven.</li>
              <li><strong>SEC/regulatory finding</strong> means the regulator formally stated findings in an enforcement proceeding. Some settlements were entered without the respondent admitting or denying those findings.</li>
              <li><strong>Dismissed</strong> and <strong>defendant prevailed</strong> outcomes are retained rather than omitted.</li>
              <li>Conduct by a <strong>portfolio company</strong> is not attributed to its investor unless a source independently establishes investor responsibility.</li>
              <li>Conduct by a parent, subsidiary, predecessor, or acquired company is identified as such rather than automatically attributed across the corporate group.</li>
              <li><strong>No strong direct matter located yet</strong> means the research conducted so far did not identify a sufficiently well-supported matter; it is not a representation that no such matter exists.</li>
            </ul>
            <p>This is a factual research record, not a determination of guilt or wrongdoing beyond what the cited matter established. Allegations, regulatory findings, settlements, dismissals, and matters involving affiliates or portfolio companies are labeled separately. Original notes are preserved; remaining citation gaps are identified beside the relevant entries.</p>
          </section>
        </div>
      </main>
      <Footer basePath={basePath} />
    </>
  );
}
