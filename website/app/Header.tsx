"use client";

import { useEffect, useRef } from "react";
import { ArrowUpRight } from "./ArrowUpRight";

export function Header({ basePath }: { basePath: string }) {
  const headerRef = useRef<HTMLElement>(null);

  useEffect(() => {
    let frame = 0;

    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const rawProgress = Math.min(Math.max(window.scrollY / 180, 0), 1);
        const progress = rawProgress * rawProgress * (3 - 2 * rawProgress);
        const logoScale = 1 - progress * 0.18;
        const glassAlpha = 0.68 - progress * 0.16;
        const shadowAlpha = progress * 0.24;

        headerRef.current?.style.setProperty("--logo-scale", logoScale.toFixed(4));
        headerRef.current?.style.setProperty("--glass-alpha", glassAlpha.toFixed(3));
        headerRef.current?.style.setProperty("--shadow-alpha", shadowAlpha.toFixed(3));
      });
    };

    update();
    window.addEventListener("scroll", update, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("scroll", update);
    };
  }, []);

  return (
    <header ref={headerRef} className="site-header">
      <div
        className="header-glass"
        aria-hidden="true"
        style={{
          backdropFilter: "blur(30px) saturate(145%) contrast(108%)",
          WebkitBackdropFilter: "blur(30px) saturate(145%) contrast(108%)",
        }}
      />
      <a className="brand" href="#top" aria-label="Amalith home">
        <img src={`${basePath}/brand/amalith-wordmark.png`} alt="Amalith" />
      </a>
      <nav className="desktop-nav" aria-label="Primary navigation">
        <a href="#why">Why Amalith</a>
        <a href="#features">Features</a>
        <a href="#status">Status</a>
        <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">GitHub</a>
      </nav>
      <a className="get-link" href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">
        Get Amalith
      </a>
      <span className="language-pill" aria-label="Language: English">EN</span>
      <details className="mobile-nav">
        <summary>Menu</summary>
        <div>
          <a href="#why">Why Amalith</a>
          <a href="#features">Features</a>
          <a href="#status">Status</a>
          <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">Get Amalith <ArrowUpRight /></a>
          <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">GitHub <ArrowUpRight /></a>
        </div>
      </details>
    </header>
  );
}
