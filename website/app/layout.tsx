import type { Metadata } from "next";
import "./globals.css";

const siteUrl = process.env.NEXT_PUBLIC_SITE_URL ?? "http://localhost:3000";
const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: "Amalith — Open-source vector design",
  description: "A free, open-source, cross-platform professional vector editor with familiar workflows and one shared command engine.",
  icons: { icon: `${basePath}/brand/favicon.svg`, shortcut: `${basePath}/brand/favicon.svg` },
  openGraph: {
    title: "Amalith — Open-source vector design",
    description: "A free, open-source professional vector editor built for familiar workflows.",
    type: "website",
    images: [{ url: `${siteUrl}/og.png`, width: 1536, height: 908, alt: "Amalith — Open-source vector design" }],
  },
  twitter: {
    card: "summary_large_image",
    title: "Amalith — Open-source vector design",
    description: "A free, open-source professional vector editor built for familiar workflows.",
    images: [`${siteUrl}/og.png`],
  },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="en"><body>{children}</body></html>;
}
