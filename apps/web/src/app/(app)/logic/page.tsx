import type { Metadata } from "next";

export const metadata: Metadata = { title: "Business logic" };

export default function LogicPage() {
  return (
    <>
      <header className="flex flex-col gap-1.5">
        <span className="text-[13px] text-subtle-foreground">Organization-wide</span>
        <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">
          Business logic
        </h1>
        <p className="max-w-[720px] text-[15px] text-muted-foreground">
          Your organization&apos;s shared business context: the entities, metrics, rules and
          relationships every project and every copilot answer is built on. Projects can add
          definitions of their own on top.
        </p>
      </header>
      <section className="flex max-w-[720px] flex-col gap-3 rounded-xl border border-dashed border-input bg-card p-8">
        <h2 className="text-[17px] font-semibold">Nothing to review yet</h2>
        <p className="text-sm leading-relaxed text-muted-foreground">
          Definitions appear here once sources are connected: Lattice profiles each source and
          proposes entities, metrics and rules. Nothing is used in an answer until someone with
          approval rights approves it, and every change shows its impact across projects first.
        </p>
      </section>
    </>
  );
}
