/** The hive mark: six hexagons around a filled centre. */
export function Logo({ size = 30 }: { size?: number }): React.ReactElement {
  const centre = 50;
  const radius = 21;
  const ring = 21;
  const points = (cx: number, cy: number, r: number): string =>
    Array.from({ length: 6 }, (_, index) => {
      const angle = (Math.PI / 180) * (60 * index - 30);
      return `${cx + r * Math.cos(angle)},${cy + r * Math.sin(angle)}`;
    }).join(" ");

  const outer = Array.from({ length: 6 }, (_, index) => {
    const angle = (Math.PI / 180) * (60 * index - 30);
    return { x: centre + ring * Math.cos(angle), y: centre + ring * Math.sin(angle) };
  });

  return (
    <svg width={size} height={size} viewBox="0 0 100 100" aria-hidden="true" focusable="false">
      <g fill="none" stroke="currentColor" strokeWidth="3.4" strokeLinejoin="round">
        {outer.map((point, index) => (
          <polygon key={index} points={points(point.x, point.y, radius)} />
        ))}
      </g>
      <polygon points={points(centre, centre, 11)} fill="currentColor" />
    </svg>
  );
}
