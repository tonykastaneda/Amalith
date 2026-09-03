import type { Metadata } from "next";
import { ArrowUpRight } from "../ArrowUpRight";
import { Header } from "../Header";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "Why Amalith",
  description: "Why Amalith is being built.",
};

export default function WhyAmalith() {
  return (
    <>
      <Header basePath={basePath} />

      <main id="top" className="why-page" aria-label="Why Amalith" />

      <footer>
        <a className="footer-brand" href={`${basePath}/`} aria-label="Amalith home">
          <img src={`${basePath}/brand/amalith-a.svg`} alt="" />
        </a>
        <div className="footer-links">
          <div>
            <p>Project</p>
            <a href={`${basePath}/why/`}>Why Amalith</a>
            <a href={`${basePath}/#why`}>Features</a>
            <a href={`${basePath}/#status`}>News</a>
          </div>
          <div>
            <p>Community</p>
            <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">GitHub <ArrowUpRight /></a>
            <a href="https://github.com/tonykastaneda/Amalith/issues" target="_blank" rel="noreferrer">Issues <ArrowUpRight /></a>
          </div>
        </div>
        <p className="footer-note">
          Built in public. Made for designers. Made with <span className="footer-heart" aria-label="love">♥</span> in Bell, California.
        </p>
      </footer>
    </>
  );
}
