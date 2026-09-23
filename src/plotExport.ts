import { plotColor } from "./theme";
import { invoke } from "@tauri-apps/api/core";
import { desktop } from "./bridge";
export async function savePlot(
  blob: Blob,
  name: string,
  format: "svg" | "png" | "html",
) {
  if (blob.size > 16 * 1024 * 1024) throw Error("Plot exceeds 16 MiB");
  name = name.replace(/[^a-z0-9_.-]+/gi, "-").slice(0, 120) + "." + format;
  if (desktop)
    return invoke<boolean>("export_plot", {
      name,
      format,
      bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
    });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return true;
}
export function svgPlot(
  svg: SVGSVGElement,
  label: string,
  detail: string,
): Blob {
  const ns = "http://www.w3.org/2000/svg";
  const root = document.createElementNS(ns, "svg");
  const { width, height } = svg.viewBox.baseVal;
  const columns = Math.max(1, Math.floor((width - 24) / 7));
  const lines = [label, detail].flatMap(
    (text) => text.match(new RegExp(`.{1,${columns}}`, "gu")) || [""],
  );
  const header = Math.max(64, lines.length * 18 + 20);
  const metadata = document.createElementNS(ns, "desc");
  metadata.textContent = `${label}\n${detail}`;
  root.append(metadata);
  root.setAttribute("width", String(width));
  root.setAttribute("height", String(height + header));
  root.setAttribute("viewBox", `0 0 ${width} ${height + header}`);
  const bg = document.createElementNS(ns, "rect");
  bg.setAttribute("width", "100%");
  bg.setAttribute("height", "100%");
  bg.setAttribute("fill", plotColor("--plot-background"));
  root.append(bg);
  for (const [index, text] of lines.entries()) {
    const y = 22 + index * 18;
    const el = document.createElementNS(ns, "text");
    el.textContent = text;
    el.setAttribute("x", "12");
    el.setAttribute("y", String(y));
    el.setAttribute("fill", plotColor("--plot-text"));
    el.setAttribute("font-family", "sans-serif");
    el.setAttribute("font-size", "12");
    root.append(el);
  }
  const copy = svg.cloneNode(true) as SVGSVGElement;
  copy.removeAttribute("class");
  copy.style.removeProperty("height");
  const originals = [svg, ...svg.querySelectorAll("*")];
  const clones = [copy, ...copy.querySelectorAll("*")];
  originals.forEach((element, index) => {
    if (!element.isConnected) return;
    const style = getComputedStyle(element);
    for (const name of [
      "fill",
      "stroke",
      "color",
      "font-size",
      "font-family",
      "font-weight",
      "opacity",
    ])
      (clones[index] as SVGElement).style.setProperty(
        name,
        style.getPropertyValue(name),
      );
  });
  copy.setAttribute("y", String(header));
  copy.setAttribute("width", String(width));
  copy.setAttribute("height", String(height));
  copy.querySelectorAll("text").forEach((el) => {
    if (!el.hasAttribute("fill"))
      el.setAttribute("fill", plotColor("--plot-muted"));
    el.setAttribute("font-size", "11");
    el.setAttribute("font-family", "monospace");
  });
  root.append(copy);
  return new Blob([new XMLSerializer().serializeToString(root)], {
    type: "image/svg+xml",
  });
}
export async function canvasPlot(
  canvas: HTMLCanvasElement,
  label: string,
  detail: string,
): Promise<Blob> {
  const ratio = canvas.width / canvas.getBoundingClientRect().width;
  const output = document.createElement("canvas");
  output.width = canvas.width;
  output.height = canvas.height + Math.ceil(64 * ratio);
  const ctx = output.getContext("2d")!;
  ctx.fillStyle = plotColor("--plot-background");
  ctx.fillRect(0, 0, output.width, output.height);
  ctx.drawImage(canvas, 0, Math.ceil(64 * ratio));
  ctx.scale(ratio, ratio);
  ctx.fillStyle = plotColor("--plot-text");
  ctx.font = "12px sans-serif";
  ctx.fillText(label, 12, 22, output.width / ratio - 24);
  ctx.fillText(detail, 12, 45, output.width / ratio - 24);
  return new Promise((resolve, reject) =>
    output.toBlob(
      (blob) => (blob ? resolve(blob) : reject(Error("Could not encode PNG"))),
      "image/png",
    ),
  );
}
export interface PlotBox {
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
  label: string;
  detail?: string;
}
export function boxPlot(
  boxes: PlotBox[],
  width: number,
  height: number,
  title: string,
  detail: string,
): Blob {
  if (boxes.length > 100000)
    throw Error("Export exceeds 100,000 boxes; narrow the scope first");
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  for (const box of boxes) {
    if (
      ![box.x, box.y, box.width, box.height].every(Number.isFinite) ||
      box.width < 0 ||
      box.height < 0
    )
      throw Error("Invalid plot geometry");
    const group = document.createElementNS(ns, "g");
    const tooltip = document.createElementNS(ns, "title");
    tooltip.textContent = box.detail || box.label;
    group.append(tooltip);
    const rect = document.createElementNS(ns, "rect");
    for (const key of ["x", "y", "width", "height"] as const)
      rect.setAttribute(key, String(box[key]));
    rect.setAttribute("fill", box.color);
    group.append(rect);
    if (box.width > 22 && box.height >= 16) {
      const text = document.createElementNS(ns, "text");
      text.textContent = box.label.slice(0, Math.floor((box.width - 10) / 7));
      text.setAttribute("x", String(box.x + 5));
      text.setAttribute("y", String(box.y + 14));
      text.setAttribute("fill", "#111820");
      group.append(text);
    }
    svg.append(group);
  }
  return svgPlot(svg, title, detail);
}
