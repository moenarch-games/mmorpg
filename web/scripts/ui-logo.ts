/**
 * The game's logo (`content/ui/logo.json`, painted natively by `mmorpg_client_model::logo`) as
 * inline SVG for the page: the opening splash, where each part draws on at its phase of the
 * shared timeline, and the plain mark the stage loader turns around. Vite writes both into
 * `index.html` when it serves or builds the page, so they paint before any script loads.
 */
type Point = [number, number];
type Phase = "ring" | "land" | "gem";
type Shape =
  | { kind: "circle"; centre: Point; radius: number; width: number; color: string; phase: Phase }
  | { kind: "line" | "outline"; points: Point[]; width: number; color: string; phase: Phase }
  | { kind: "fill"; points: Point[]; color: string; phase: Phase };
type Logo = {
  schema_version: number;
  title: string;
  subtitle: string;
  timeline_ms: Record<Phase | "title", [number, number]> & { hold: number; fade: number };
  shapes: Shape[];
};

function parse(json: string): Logo {
  const logo = JSON.parse(json) as Logo;
  if (logo.schema_version !== 1) { throw new Error(`logo schema ${logo.schema_version} is not 1`); }
  return logo;
}

const escape = (text: string) => text.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
const colour = (name: string) => {
  if (!/^[a-z_]+$/.test(name)) { throw new Error(`logo colour ${name} is not a theme colour name`); }
  return `var(--ui-color-${name.replaceAll("_", "-")})`;
};
const points = (list: Point[]) => list.map(([x, y]) => `${x},${y}`).join(" ");

/** The logo as SVG; `animated` adds each part's draw-on timing from the shared timeline. */
export function logoSvg(json: string, animated: boolean): string {
  const logo = parse(json);
  const shapes = logo.shapes.map((shape) => {
    const [start, end] = logo.timeline_ms[shape.phase];
    const timing = animated ? ` style="--start:${start}ms;--length:${end - start}ms"` : "";
    switch (shape.kind) {
      case "circle":
        return `<circle class="logo-draw"${timing} cx="${shape.centre[0]}" cy="${shape.centre[1]}" r="${shape.radius}" fill="none" stroke="${colour(shape.color)}" stroke-width="${shape.width}" pathLength="1" transform="rotate(-90 ${shape.centre[0]} ${shape.centre[1]})"/>`;
      case "line":
      case "outline":
        return `<polyline class="logo-draw"${timing} points="${points(shape.points)}" fill="none" stroke="${colour(shape.color)}" stroke-width="${shape.width}" stroke-linecap="round" stroke-linejoin="round" pathLength="1"/>`;
      case "fill":
        return `<polygon class="logo-gem"${timing} points="${points(shape.points)}" fill="${colour(shape.color)}"/>`;
    }
  });
  return `<svg class="logo" viewBox="0 0 100 100" role="img" aria-label="${escape(logo.title)}">${shapes.join("")}</svg>`;
}

/** The opening splash: the drawn-on logo, the title and subtitle, then the fade at the hold. */
export function splashHtml(json: string): string {
  const logo = parse(json);
  const { title, hold, fade } = logo.timeline_ms;
  const titleTiming = `--start:${title[0]}ms;--length:${title[1] - title[0]}ms`;
  return `<div id="splash" class="splash" style="--hold:${hold}ms;--fade:${fade}ms" aria-hidden="true">`
    + `<div class="splash-mark">${logoSvg(json, true)}</div>`
    + `<p class="splash-title" style="${titleTiming}">${escape(logo.title)}</p>`
    + `<p class="splash-subtitle" style="${titleTiming}">${escape(logo.subtitle)}</p>`
    + `</div>`;
}
