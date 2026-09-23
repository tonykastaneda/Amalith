"use client";

import { useEffect, useState } from "react";

const REPO = "tonykastaneda/Amalith";
// Always valid regardless of whether any release is tagged "latest" (a
// GitHub concept that explicitly excludes pre-releases) — the safe link
// before the client fetch below resolves, and if it fails.
const RELEASES_PAGE = `https://github.com/${REPO}/releases`;

export type Platform = {
  name: string;
  icon: string;
  label: string;
  /**
   * File extension (e.g. ".dmg") that picks this platform's asset out of
   * the newest release's asset list, or null to always fall back to the
   * release page instead of a direct asset link.
   */
  matchExt: string | null;
};

type ReleaseAsset = { name: string; browser_download_url: string };
type Release = { assets: ReleaseAsset[]; html_url: string; draft: boolean };

export function DownloadLinks({ platforms }: { platforms: Platform[] }) {
  const [hrefs, setHrefs] = useState<Record<string, string>>({});

  useEffect(() => {
    let cancelled = false;
    // GitHub's own "latest release" endpoint skips pre-releases, which is
    // all this pre-alpha repo has — so fetch the raw list (newest first)
    // and take the first non-draft entry instead.
    fetch(`https://api.github.com/repos/${REPO}/releases`)
      .then((res) => (res.ok ? (res.json() as Promise<Release[]>) : Promise.reject(res.status)))
      .then((releases) => {
        const latest = releases.find((r) => !r.draft);
        if (!latest || cancelled) return;
        const next: Record<string, string> = {};
        for (const platform of platforms) {
          const asset = platform.matchExt
            ? latest.assets.find((a) => a.name.toLowerCase().endsWith(platform.matchExt!))
            : undefined;
          next[platform.name] = asset?.browser_download_url ?? latest.html_url;
        }
        setHrefs(next);
      })
      .catch(() => {
        // Leave the RELEASES_PAGE fallback in place — unauthenticated
        // GitHub API calls are rate-limited, and this degrades gracefully.
      });
    return () => {
      cancelled = true;
    };
  }, [platforms]);

  return (
    <div className="platform-grid">
      {platforms.map((platform) => (
        <article className="platform-card" key={platform.name}>
          <div className="platform-card__icon">
            <img src={platform.icon} alt="" aria-hidden="true" />
          </div>
          <h3>{platform.name}</h3>
          <a className="availability-label" href={hrefs[platform.name] ?? RELEASES_PAGE}>
            {platform.label}
          </a>
        </article>
      ))}
    </div>
  );
}
