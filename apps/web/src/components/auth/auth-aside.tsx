import { LogoMark } from "@/components/brand/logo";

/** Static, server-rendered brand panel (no JS needed to paint it). */
export function AuthAside() {
  return (
    <section
      aria-label="About Lattice"
      className="flex min-w-0 flex-[1_1_520px] flex-col justify-between gap-12 bg-brand-ink px-16 py-14 text-brand-text max-sm:px-8"
    >
      <div className="flex items-center gap-2.5">
        <LogoMark size={28} />
        <span className="text-[19px] font-semibold tracking-[-0.01em]">Lattice</span>
      </div>

      <div className="flex max-w-[520px] flex-col gap-7">
        <h1 className="text-5xl leading-[1.05] font-semibold tracking-[-0.03em] text-white max-sm:text-4xl">
          Ask your data.
          <br />
          Keep the logic.
        </h1>
        <p className="text-[17px] leading-[1.55] text-brand-muted">
          Connect databases, APIs and spreadsheets. Lattice&apos;s agents learn how your business
          defines its numbers, build the dashboards, and keep every answer traceable.
        </p>

        <div className="flex flex-col gap-3 rounded-[14px] border border-brand-line bg-brand-panel p-[18px]">
          <div className="flex items-start gap-2.5">
            <span className="flex size-7 flex-none items-center justify-center rounded-full bg-[#2a3250] text-[11px] font-semibold">
              PR
            </span>
            <p className="mt-1 text-sm">What was net revenue by region last quarter?</p>
          </div>
          <div className="flex items-start gap-2.5">
            <span className="flex size-7 flex-none items-center justify-center rounded-lg bg-primary">
              <SparkleIcon />
            </span>
            <div className="flex min-w-0 flex-col gap-2">
              <p className="mt-1 text-sm leading-normal text-[#c9cfdd]">
                Built a bar chart using your approved{" "}
                <span className="font-medium text-white">Net revenue</span> definition — settled
                orders minus refunds, fiscal Q3 (Aug–Oct).
              </p>
              <div className="flex flex-wrap gap-1.5">
                {["Sales warehouse", "Stripe API"].map((source) => (
                  <span
                    key={source}
                    className="rounded-md bg-brand-chip px-2 py-[3px] font-mono text-[11px] text-brand-muted"
                  >
                    {source}
                  </span>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      <p className="text-[13px] text-brand-faint">
        Governed business logic · Human-approved definitions · Every answer traceable
      </p>
    </section>
  );
}

export function SparkleIcon({ size = 15, color = "#FFFFFF" }: { size?: number; color?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={color}
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" />
    </svg>
  );
}
