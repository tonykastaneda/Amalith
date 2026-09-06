import type { ReactNode } from "react";
import { Header } from "./Header";
import { Footer } from "./Footer";
import { MarketingHero } from "./MarketingHero";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";
const pages = [
  { slug: "privacy", label: "Privacy & cookies" },
  { slug: "terms", label: "Terms & payments" },
  { slug: "contact", label: "Contact & accessibility" },
];

export function PolicyLayout({ current, title, intro, children }: {
  current: string;
  title: ReactNode;
  intro: string;
  children: ReactNode;
}) {
  return (
    <>
      <Header basePath={basePath} />
      <main id="top" tabIndex={-1} className="marketing-page information-page">
        <MarketingHero eyebrow="Amalith / Project information" title={title}>
          <p>{intro}</p>
        </MarketingHero>
        <div className="information-layout">
          <nav className="information-nav" aria-label="Project information">
            {pages.map(({ slug, label }) => (
              <a key={slug} href={`${basePath}/${slug}/`} aria-current={slug === current ? "page" : undefined}>{label}</a>
            ))}
          </nav>
          <article className="information-body">{children}</article>
        </div>
      </main>
      <Footer basePath={basePath} />
    </>
  );
}
