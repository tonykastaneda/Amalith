import type { Metadata } from "next";
import "./globals.css";

const siteUrl = process.env.NEXT_PUBLIC_SITE_URL ?? "http://localhost:3000";
const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: "Amalith — The Open Source Design Suite",
  description: "A free, open-source, cross-platform professional vector editor with familiar workflows and one shared command engine.",
  icons: { icon: `${basePath}/brand/favicon.svg`, shortcut: `${basePath}/brand/favicon.svg` },
  openGraph: {
    title: "Amalith — The Open Source Design Suite",
    description: "A free, open-source professional vector editor built for familiar workflows.",
    type: "website",
    images: [{ url: `${siteUrl}/og-suite.png`, width: 1730, height: 909, alt: "Amalith — The Open Source Design Suite" }],
  },
  twitter: {
    card: "summary_large_image",
    title: "Amalith — The Open Source Design Suite",
    description: "A free, open-source professional vector editor built for familiar workflows.",
    images: [`${siteUrl}/og-suite.png`],
  },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="en"><body>{children}</body></html>;
}
