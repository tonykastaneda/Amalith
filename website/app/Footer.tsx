import { ArrowUpRight } from "./ArrowUpRight";

export function Footer({ basePath }: { basePath: string }) {
  return (
    <footer>
      <a className="footer-brand" href={`${basePath}/#top`} aria-label="Amalith home">
        <img src={`${basePath}/brand/amalith-a.svg`} alt="" />
      </a>
      <div className="footer-links">
        <div>
          <p>Project</p>
          <a href={`${basePath}/why/`}>Why Amalith</a>
          <a href={`${basePath}/#features`}>Features</a>
          <a href={`${basePath}/#status`}>News</a>
          <a href={`${basePath}/docs/`}>Docs</a>
        </div>
        <div>
          <p>Community &amp; policies</p>
          <a href={`${basePath}/privacy/`}>Privacy &amp; cookies</a>
          <a href={`${basePath}/terms/`}>Terms &amp; payments</a>
          <a href={`${basePath}/contact/`}>Contact &amp; accessibility</a>
          <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">GitHub <ArrowUpRight /></a>
          <a href="https://github.com/tonykastaneda/Amalith/issues" target="_blank" rel="noreferrer">Issues <ArrowUpRight /></a>
        </div>
      </div>
      <p className="footer-note">
        Built in public. Made for designers. <span className="footer-heart" aria-label="love">♥</span> from Bell, California.
      </p>
    </footer>
  );
}
