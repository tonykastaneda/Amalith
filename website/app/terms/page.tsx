import type { Metadata } from "next";
import { PolicyLayout } from "../PolicyLayout";

export const metadata: Metadata = { title: "Terms — Amalith" };

export default function Page() {
  return (
    <PolicyLayout current="terms" title="Terms" intro="Free. No plans to ever charge.">
      <p>Last updated: September 22, 2026.</p>

      <h2>Free, with no plans to charge</h2>
      <p>Amalith is free to use, and we have no plans to ever charge for it. No payment, card, subscription, or purchase is required. This website does not accept payments or create a paid subscription.</p>

      <h2>Refunds and cancellations</h2>
      <p>This website does not process payments, so there is nothing to refund or cancel. Nothing on this page limits rights you have under applicable law.</p>

      <h2>Development status</h2>
      <p>Amalith is in active development. Features, compatibility, and release plans can change. Illustrative website previews are labeled as placeholders. Keep backups of important work and consult the repository for current build instructions and limitations.</p>

      <h2>Software and third-party rights</h2>
      <p>The repository identifies Amalith’s software license as MIT OR Apache-2.0. Consult the license notices accompanying the version you use; this website does not replace those licenses. Third-party names and marks belong to their respective owners. References to other products do not imply endorsement or affiliation.</p>

      <h2>External links and contact</h2>
      <p>External services have their own terms and privacy practices. For project questions, see <a href="../contact/">Contact &amp; accessibility</a>. Website privacy information is on our <a href="../privacy/">Privacy &amp; cookies</a> page.</p>
    </PolicyLayout>
  );
}
