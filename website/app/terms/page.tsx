import type { Metadata } from "next";
import { PolicyLayout } from "../PolicyLayout";

export const metadata: Metadata = { title: "Terms & payments — Amalith" };

export default function Page() {
  return (
    <PolicyLayout current="terms" title="Terms & payments" intro="Free today. Clear about what comes next.">
      <p>Last updated: September 6, 2026.</p>

      <h2>Free today</h2>
      <p>Amalith is free to use today. No payment, card, subscription, or purchase is required. This website does not accept payments or create a paid subscription.</p>

      <h2>Future pricing</h2>
      <p>We plan to charge for Amalith in the future, but have not announced a date, price, or which offerings will be paid. Free today is not a promise that every future version or service will remain free. Any paid offer will describe its price and applicable terms before you choose to buy it. Visiting this site or using the current free app does not authorize a future charge.</p>

      <h2>Refunds and cancellations</h2>
      <p>There are currently no purchases or paid subscriptions through this website to refund or cancel. Before accepting payments, we will publish the applicable purchase, cancellation, and refund terms. Nothing on this page limits rights you have under applicable law.</p>

      <h2>Development status</h2>
      <p>Amalith is in active development. Features, compatibility, and release plans can change. Illustrative website previews are labeled as placeholders. Keep backups of important work and consult the repository for current build instructions and limitations.</p>

      <h2>Software and third-party rights</h2>
      <p>The repository identifies Amalith’s software license as MIT OR Apache-2.0. Consult the license notices accompanying the version you use; this website does not replace those licenses. Third-party names and marks belong to their respective owners. References to other products do not imply endorsement or affiliation.</p>

      <h2>External links and contact</h2>
      <p>External services have their own terms and privacy practices. For project questions, see <a href="../contact/">Contact &amp; accessibility</a>. Website privacy information is on our <a href="../privacy/">Privacy &amp; cookies</a> page.</p>
    </PolicyLayout>
  );
}
