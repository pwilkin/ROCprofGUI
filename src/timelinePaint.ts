import type { TimelineGlyph } from "./timelineDetail";

export function paintTimelineGlyphs(
  ctx: CanvasRenderingContext2D,
  glyphs: TimelineGlyph[],
  overlays: TimelineGlyph[],
  selectionColor: () => string,
) {
  let color: string | undefined;
  let alpha: number | undefined;
  let selection: string | undefined;
  for (const group of [glyphs, overlays]) {
    for (const { mark, rect } of group) {
      const nextAlpha = mark.selected || group === overlays ? 1 : 0.75;
      if (color !== mark.color) {
        ctx.fillStyle = mark.color;
        color = mark.color;
      }
      if (alpha !== nextAlpha) {
        ctx.globalAlpha = nextAlpha;
        alpha = nextAlpha;
      }
      ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
      if (mark.selected) {
        if (selection === undefined) {
          selection = selectionColor();
          ctx.strokeStyle = selection;
        }
        ctx.strokeRect(rect.x, rect.y, rect.width, rect.height);
      }
    }
  }
  ctx.globalAlpha = 1;
}
