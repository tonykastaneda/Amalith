import type { Metadata } from "next";
import { Footer } from "../Footer";
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

      <Footer basePath={basePath} />
    </>
  );
}
