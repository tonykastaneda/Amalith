import type { Metadata } from "next";
import { Footer } from "../Footer";
import { Header } from "../Header";
import { DownloadLinks, type Platform } from "./DownloadLinks";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  title: "Download Amalith",
  description: "Download Amalith for macOS, Windows 10/11, and Linux.",
};

const platforms: Platform[] = [
  {
    name: "macOS",
    icon: `${basePath}/brand/platform/mac.svg`,
    label: "Download for macOS",
    matchExt: ".dmg",
  },
  {
    name: "Windows",
    icon: `${basePath}/brand/platform/win.svg`,
    label: "Download for Windows",
    // Must stay more specific than ".zip": Linux also ships a .zip now, and
    // the asset lookup takes the first match — "Amalith-Linux.zip" sorts
    // ahead of "Amalith-Windows.zip", so a bare ".zip" would hand Windows
    // users the Linux download.
    matchExt: "-windows.zip",
  },
  {
    name: "Linux",
    icon: `${basePath}/brand/platform/linux.svg`,
    label: "Download for Linux",
    // One zip now carries every install method — AppImage, deb, rpm,
    // portable tarball and the Arch PKGBUILD — with an INSTALL.txt giving
    // the command for each, so there's a single asset to link rather than
    // four for the user to choose between on the release page.
    matchExt: "-linux.zip",
  },
];

export default function Downloads() {
  return (
    <>
      <Header basePath={basePath} />

      <main id="top" className="downloads-page">
        <section className="downloads-hero" aria-labelledby="downloads-title">
          <div className="downloads-hero__inner">
            <p className="kicker"><span /> Finally</p>
            <h1 id="downloads-title">
              <span>The design tool that lets</span>
              <span>creatives <em>create.</em></span>
            </h1>
            <DownloadLinks platforms={platforms} />
            <p className="downloads-disclaimer">
              Amalith is currently in alpha and isn&rsquo;t ready for prime time yet. If you&rsquo;d like to contribute, visit the <a href="https://github.com/tonykastaneda/Amalith" target="_blank" rel="noreferrer">Amalith GitHub repository</a>.
            </p>
          </div>
        </section>
      </main>

      <Footer basePath={basePath} />
    </>
  );
}
