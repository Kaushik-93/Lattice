import { cn } from "@/lib/utils";

export function LogoMark({ size = 26, className }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" aria-hidden="true" className={className}>
      <rect x="1" y="1" width="26" height="26" rx="7" fill="#4338CA" />
      <path
        d="M8 9h12M8 14h12M8 19h12M10 7v14M14 7v14M18 7v14"
        stroke="#FFFFFF"
        strokeWidth="1.6"
        strokeLinecap="round"
        opacity="0.9"
      />
    </svg>
  );
}

export function Logo({ size = 26, className }: { size?: number; className?: string }) {
  return (
    <span className={cn("flex items-center gap-2.5", className)}>
      <LogoMark size={size} />
      <span className="text-[17px] font-semibold tracking-[-0.01em]">Lattice</span>
    </span>
  );
}
