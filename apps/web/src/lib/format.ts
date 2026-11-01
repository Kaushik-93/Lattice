const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "Updated 4 min ago" / "Updated yesterday" / "Updated Oct 28", as on the design's cards. */
export function updatedLabel(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  const seconds = Math.max(0, Math.round((now.getTime() - then.getTime()) / 1000));
  if (seconds < 60) return "Updated just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `Updated ${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24 && then.getDate() === now.getDate()) return `Updated ${hours} h ago`;

  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (then.toDateString() === yesterday.toDateString()) return "Updated yesterday";

  const sameYear = then.getFullYear() === now.getFullYear();
  return `Updated ${MONTHS[then.getMonth()]} ${then.getDate()}${sameYear ? "" : `, ${then.getFullYear()}`}`;
}

/** "now", "4 min ago", "3 h ago", "Oct 28" — for "last sync" columns. */
export function timeAgo(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return "—";
  const label = updatedLabel(iso, now).replace(/^Updated /, "");
  return label === "just now" ? "now" : label;
}

export function formatCount(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  return new Intl.NumberFormat("en-US").format(n);
}

/** 1,050,060 → "1.05M"; 18,200 → "18.2K". */
export function compactCount(n: number): string {
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 }).format(
    n,
  );
}

export function capitalize(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1);
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`;
}
