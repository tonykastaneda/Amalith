import type { Metadata } from "next";
import { ArrowUpRight } from "../ArrowUpRight";
import { Footer } from "../Footer";
import { Header } from "../Header";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "Amalith Docs",
  description: "Documentation for Amalith, the open-source professional vector editor.",
};

const navigation = [
  "Getting started",
  "Workspace",
  "Tools",
  "Objects & layers",
  "Text",
  "File formats",
  "Scripting",
  "Help",
];

const featured = [
  {
    title: "Project overview",
    body: "Understand Amalith’s goals, architecture, and the product principles guiding the editor.",
    href: "https://github.com/tonykastaneda/Amalith/blob/main/amalith-project-brief.md",
  },
  {
    title: "Build from source",
    body: "Set up the Rust toolchain, run the desktop app, and explore the project locally.",
    href: "https://github.com/tonykastaneda/Amalith#build-and-run",
  },
  {
    title: "Text tool",
    body: "Read about point type, area type, editing behavior, and the text engine’s design.",
    href: "https://github.com/tonykastaneda/Amalith/blob/main/docs/text-tool.md",
  },
  {
    title: "Core architecture",
    body: "Learn how documents, commands, file I/O, and the native shell fit together.",
    href: "https://github.com/tonykastaneda/Amalith#crates",
  },
];

export default function Docs() {
  return (
    <>
      <Header basePath={basePath} />

      <div className="docs-page">
        <aside className="docs-sidebar" aria-label="Documentation navigation">
          <nav>
            <a className="docs-sidebar__active" href={`${basePath}/docs/`}>Amalith Docs</a>
            <a href={`${basePath}/why/`}>About Amalith</a>
            <div className="docs-sidebar__rule" />
            {navigation.map((item) => (
              <span className="docs-sidebar__group" key={item}>{item}<b aria-hidden="true">⌄</b></span>
            ))}
          </nav>
        </aside>

        <main className="docs-main" id="top">
          <article className="docs-article">
            <div className="docs-breadcrumb"><a href={`${basePath}/docs/`}>Amalith Docs</a></div>

            <header className="docs-heading">
              <p className="section-number">Documentation</p>
              <h1>Amalith Docs</h1>
              <p>Learn the workspace, tools, concepts, and workflows behind Amalith—the free, open-source professional vector editor.</p>
            </header>

            <section className="docs-section" id="get-started">
              <h2>Get Started <a href="#get-started" aria-label="Link to Get Started">#</a></h2>
              <p>Amalith is in early development, but the native editor is already ready to explore. You can follow the project, build it locally, and help shape what comes next.</p>

              <h3 id="installation">Installation</h3>
              <p>Ready-to-run builds for macOS, Windows, and Linux are coming soon. Until then, developers can build Amalith directly from the source repository.</p>
              <div className="docs-actions">
                <a className="docs-action docs-action--primary" href={`${basePath}/downloads/`}>Downloads</a>
                <a className="docs-action" href="https://github.com/tonykastaneda/Amalith#build-and-run" target="_blank" rel="noreferrer">Build from source <ArrowUpRight /></a>
              </div>
            </section>

            <hr />

            <section className="docs-section" id="featured">
              <h2>Featured Documentation <a href="#featured" aria-label="Link to Featured Documentation">#</a></h2>
              <div className="docs-featured">
                {featured.map((item) => (
                  <a href={item.href} target="_blank" rel="noreferrer" key={item.title}>
                    <h3>{item.title}</h3>
                    <p>{item.body}</p>
                    <span>Read on GitHub <ArrowUpRight /></span>
                  </a>
                ))}
              </div>
            </section>

            <a className="docs-edit" href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">Edit on GitHub <ArrowUpRight /></a>
          </article>

          <aside className="docs-on-page" aria-label="On this page">
            <p>On this page</p>
            <a href="#get-started">Get Started</a>
            <a href="#installation">Installation</a>
            <a href="#featured">Featured Documentation</a>
          </aside>
        </main>
      </div>

      <Footer basePath={basePath} />
    </>
  );
}
