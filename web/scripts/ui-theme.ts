/**
 * The shared interface theme (`content/ui/theme.json`, read natively by
 * `mmorpg_client_model::theme`) as CSS custom properties: `--ui-color-gold`,
 * `--ui-radius-panel`, `--ui-space-md`, `--ui-text-body`, `--ui-motion-fast`.
 * Vite writes the result to `src/generated/ui-theme.css` when it starts.
 */
type Group = Record<string, string | number>;
type ThemeFile = { schema_version: number } & Record<string, Group | number>;

const UNITS: Record<string, string> = { radius: "px", space: "px", text: "px", motion_ms: "ms" };

export function themeCss(json: string): string {
  const theme = JSON.parse(json) as ThemeFile;
  if (theme.schema_version !== 1) { throw new Error(`theme schema ${theme.schema_version} is not 1`); }
  const lines: string[] = [];
  for (const [group, values] of Object.entries(theme)) {
    if (group === "schema_version" || typeof values !== "object") { continue; }
    const prefix = group === "motion_ms" ? "motion" : group;
    for (const [name, value] of Object.entries(values)) {
      if (group === "color") {
        if (typeof value !== "string" || !/^#([0-9a-f]{6}|[0-9a-f]{8})$/i.test(value)) {
          throw new Error(`color.${name} is not #rrggbb or #rrggbbaa`);
        }
        lines.push(`  --ui-color-${name.replaceAll("_", "-")}: ${value};`);
      } else {
        const unit = UNITS[group];
        if (!unit || typeof value !== "number") { throw new Error(`${group}.${name} is not a known number`); }
        lines.push(`  --ui-${prefix}-${name.replaceAll("_", "-")}: ${value}${unit};`);
      }
    }
  }
  return `/* Generated from content/ui/theme.json by web/scripts/ui-theme.ts. */\n:root {\n${lines.join("\n")}\n}\n`;
}
