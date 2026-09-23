import { ArrowUpRight } from "./ArrowUpRight";
import "./footer.css";

export function Footer({ basePath }: { basePath: string }) {
  return (
    <footer className="amalith-footer">
      <div className="footer-links">
        <div>
          <p>Project</p>
          <a href={`${basePath}/why/`}>Why Amalith</a>
          <a href={`${basePath}/#features`}>Features</a>
          <a href={`${basePath}/#status`}>News</a>
          <a href={`${basePath}/docs/`}>Docs</a>
        </div>
        <div>
          <p>Downloads</p>
          <a href={`${basePath}/downloads/`}>Get Amalith</a>
        </div>
        <div>
          <p>Community</p>
          <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">GitHub <ArrowUpRight /></a>
          <a href="https://github.com/tonykastaneda/Amalith/issues" target="_blank" rel="noreferrer">Issues <ArrowUpRight /></a>
          <a href="https://tonykastaneda.github.io/GlyphClub/" target="_blank" rel="noreferrer">Our Font Manager <ArrowUpRight /></a>
        </div>
        <div>
          <p>Legal</p>
          <a href={`${basePath}/privacy/`}>Privacy &amp; cookies</a>
          <a href={`${basePath}/terms/`}>Terms</a>
          <a href={`${basePath}/contact/`}>Contact &amp; accessibility</a>
        </div>
      </div>
      <p className="footer-note">
        <span>© 2026 Amalith</span>
        <span>Built in public. Made for designers. <span className="footer-heart" aria-label="love">♥</span> from Bell, California.</span>
      </p>
      <a className="footer-wordmark" href={`${basePath}/#top`} aria-label="Amalith home">
        <img src={`${basePath}/brand/amalith-wordmark-text.svg`} alt="Amalith" width="644" height="124" />
      </a>
    </footer>
  );
}
