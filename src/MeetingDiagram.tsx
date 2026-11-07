import { useEffect, useRef, useState } from "react";
import type { RecordDiagram } from "./clientApi";

export function MeetingDiagram({ diagram }: { diagram: RecordDiagram }) {
  const surface = useRef<HTMLDivElement>(null);
  const expandedSurface = useRef<HTMLDivElement>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const [scene, setScene] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false;
    setScene("");
    setError("");
    void (async () => {
      const { convertToExcalidrawElements, exportToSvg, serializeAsJSON } =
        await import("@excalidraw/excalidraw");
      const ranks = new Map(diagram.nodes.map((node) => [node.id, 0]));
      for (let round = 0; round < diagram.nodes.length; round++) {
        let changed = false;
        for (const edge of diagram.edges) {
          const rank = Math.min(
            diagram.nodes.length - 1,
            (ranks.get(edge.from) ?? 0) + 1,
          );
          if (rank > (ranks.get(edge.to) ?? 0)) {
            ranks.set(edge.to, rank);
            changed = true;
          }
        }
        if (!changed) break;
      }
      const levels = Array.from(new Set(ranks.values())).sort((a, b) => a - b);
      const positions = new Map<string, { x: number; y: number }>();
      let levelTop = 0;
      for (const level of levels) {
        const nodes = diagram.nodes.filter(
          (node) => ranks.get(node.id) === level,
        );
        nodes.forEach((node, index) =>
          positions.set(node.id, {
            x: nodes.length === 1 ? 130 : (index % 2) * 260,
            y: levelTop + Math.floor(index / 2) * 140,
          }),
        );
        levelTop += Math.ceil(nodes.length / 2) * 140;
      }
      const skeleton: NonNullable<
        Parameters<typeof convertToExcalidrawElements>[0]
      > = [];
      const colors = ["#edf4ff", "#edf7ef", "#fff4e5", "#f3edfb"];
      for (const edge of diagram.edges) {
        const from = positions.get(edge.from),
          to = positions.get(edge.to);
        if (!from || !to) continue;
        const dx = to.x - from.x,
          dy = to.y - from.y - 80;
        skeleton.push({
          type: "arrow",
          x: from.x + 110,
          y: from.y + 80,
          points: [
            [0, 0],
            [dx, dy],
          ],
          strokeColor: "#8394a3",
          strokeWidth: 1,
          roughness: 0,
          ...(edge.label
            ? {
                label: {
                  text: edge.label,
                  fontSize: 13,
                  fontFamily: 2,
                  strokeColor: "#57646d",
                },
              }
            : {}),
        });
      }
      diagram.nodes.forEach((node, index) => {
        const position = positions.get(node.id)!;
        skeleton.push({
          type: "rectangle",
          id: node.id,
          x: position.x,
          y: position.y,
          width: 220,
          height: 80,
          roundness: { type: 3 },
          backgroundColor: colors[index % colors.length],
          fillStyle: "solid",
          strokeColor: "#a1b0b8",
          strokeWidth: 1,
          roughness: 0,
          label: {
            text: node.label,
            fontSize: 16,
            fontFamily: 2,
            strokeColor: "#283a44",
          },
        });
      });
      const elements = convertToExcalidrawElements(skeleton);
      const svg = await exportToSvg({
        elements,
        files: {},
        appState: { exportBackground: false },
        skipInliningFonts: true,
        exportPadding: 24,
      });
      if (disposed) return;
      svg.setAttribute("role", "img");
      svg.setAttribute("aria-label", "内容关系图");
      svg.style.width = "100%";
      svg.style.height = "auto";
      surface.current?.replaceChildren(svg);
      expandedSurface.current?.replaceChildren(svg.cloneNode(true));
      setScene(
        serializeAsJSON(
          elements,
          { viewBackgroundColor: "#ffffff" },
          {},
          "local",
        ),
      );
    })().catch((reason) => {
      if (!disposed)
        setError(reason instanceof Error ? reason.message : String(reason));
    });
    return () => {
      disposed = true;
    };
  }, [diagram]);
  function download() {
    const url = URL.createObjectURL(
      new Blob([scene], { type: "application/json" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = "会议关系图.excalidraw";
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return (
    <figure className="meeting-diagram">
      <figcaption>
        <strong>内容关系图</strong>
        <span>Excalidraw</span>
        <button
          className="ghost small"
          disabled={!scene}
          onClick={() => dialog.current?.showModal()}
        >
          放大
        </button>
        <button className="ghost small" disabled={!scene} onClick={download}>
          下载图稿
        </button>
      </figcaption>
      {error ? (
        <p role="status">关系图暂时无法显示。</p>
      ) : (
        !scene && <p>正在绘图…</p>
      )}
      <div ref={surface} className="meeting-diagram-surface" />
      <dialog ref={dialog} className="meeting-diagram-dialog">
        <header>
          <strong>内容关系图</strong>
          <button onClick={() => dialog.current?.close()}>关闭</button>
        </header>
        <div ref={expandedSurface} />
      </dialog>
    </figure>
  );
}
