/** The coloured initial tile used for projects throughout the design. */
export function ProjectTile({
  name,
  color,
  size = 36,
}: {
  name: string;
  color: string;
  size?: number;
}) {
  return (
    <span
      aria-hidden="true"
      className="flex flex-none items-center justify-center font-semibold text-white"
      style={{
        width: size,
        height: size,
        background: color,
        borderRadius: size >= 36 ? Math.round(size * 0.28) : 6,
        // Design: 20px → 10px, 22px → 11px, 36px → 15px, 52px → 22px.
        fontSize: Math.round(size * (size < 30 ? 0.5 : 0.42)),
      }}
    >
      {name.trim().charAt(0).toUpperCase()}
    </span>
  );
}
