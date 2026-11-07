import { useCallback, useEffect, useRef } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import {
  currentMonitor,
  getCurrentWindow,
  monitorFromPoint,
  primaryMonitor,
} from "@tauri-apps/api/window";
import type { Monitor } from "@tauri-apps/api/window";

type Bounds = { position: PhysicalPosition; size: PhysicalSize };
const compactWidth = 1040;
const panelSize = { width: 1040, height: 540 };

async function monitorForWindow() {
  return (await currentMonitor()) ?? (await primaryMonitor());
}

function windowBounds(
  monitor: Monitor,
  size: { width: number; height: number },
  anchor?: Bounds,
): Bounds {
  const { position, size: area } = monitor.workArea;
  const inset = Math.round(10 * monitor.scaleFactor);
  const width = Math.min(area.width - 2 * inset, size.width);
  const height = Math.min(area.height - 2 * inset, size.height);
  const x = anchor
    ? anchor.position.x + (anchor.size.width - width) / 2
    : position.x + (area.width - width) / 2;
  const y = anchor
    ? anchor.position.y + anchor.size.height - height
    : position.y + area.height - inset - height;
  return {
    size: new PhysicalSize(width, height),
    position: new PhysicalPosition(
      Math.round(
        Math.max(
          position.x + inset,
          Math.min(x, position.x + area.width - inset - width),
        ),
      ),
      Math.round(
        Math.max(
          position.y + inset,
          Math.min(y, position.y + area.height - inset - height),
        ),
      ),
    ),
  };
}

function pixels(size: { width: number; height: number }, scale: number) {
  return {
    width: Math.round(size.width * scale),
    height: Math.round(size.height * scale),
  };
}

async function setBounds(bounds: Bounds) {
  const window = getCurrentWindow();
  await window.setPosition(bounds.position);
  await window.setSize(bounds.size);
}

export function useCaptionWindow(
  expanded: boolean,
  compactHeight: number,
  reportError: (message: string) => void,
) {
  const queue = useRef(Promise.resolve());
  const compactBounds = useRef<(Bounds & { preferredHeight: number }) | null>(
    null,
  );
  const previousMode = useRef<{
    expanded: boolean;
    compactHeight: number;
  } | null>(null);
  const enqueue = useCallback(
    (task: () => Promise<void>) => {
      queue.current = queue.current
        .then(task)
        .catch(() => reportError("无法调整字幕窗口的位置或大小。"));
    },
    [reportError],
  );

  const returnToBottom = useCallback(() => {
    if (!isTauri()) return;
    enqueue(async () => {
      const monitor = await monitorForWindow();
      if (monitor)
        await setBounds(
          windowBounds(
            monitor,
            pixels(
              { width: compactWidth, height: compactHeight },
              monitor.scaleFactor,
            ),
          ),
        );
    });
  }, [enqueue, compactHeight]);

  useEffect(() => {
    if (!isTauri()) return;
    const previous = previousMode.current;
    if (
      previous?.expanded === expanded &&
      previous.compactHeight === compactHeight
    )
      return;
    previousMode.current = { expanded, compactHeight };
    if (previous?.expanded && expanded) return;

    enqueue(async () => {
      const window = getCurrentWindow();
      if (previous === null) {
        try {
          const monitor = await monitorForWindow();
          if (monitor)
            await setBounds(
              windowBounds(
                monitor,
                pixels(
                  { width: compactWidth, height: compactHeight },
                  monitor.scaleFactor,
                ),
              ),
            );
        } finally {
          await window.show();
        }
        if (!expanded) return;
      }

      if (expanded) {
        const monitor = await monitorForWindow();
        if (!monitor) return;
        const [position, size] = await Promise.all([
          window.outerPosition(),
          window.innerSize(),
        ]);
        compactBounds.current = {
          position,
          size,
          preferredHeight: previous?.compactHeight ?? compactHeight,
        };
        await setBounds(
          windowBounds(
            monitor,
            pixels(panelSize, monitor.scaleFactor),
            compactBounds.current,
          ),
        );
        return;
      }

      const anchor = previous?.expanded
        ? compactBounds.current
        : {
            position: await window.outerPosition(),
            size: await window.innerSize(),
            preferredHeight: previous?.compactHeight ?? compactHeight,
          };
      compactBounds.current = null;
      const originalMonitor = anchor
        ? await monitorFromPoint(
            anchor.position.x + anchor.size.width / 2,
            anchor.position.y + anchor.size.height / 2,
          )
        : null;
      const monitor = originalMonitor ?? (await monitorForWindow());
      if (!monitor) return;
      const preferredSize = pixels(
        { width: compactWidth, height: compactHeight },
        monitor.scaleFactor,
      );
      const restore = originalMonitor && anchor ? anchor : undefined;
      const size = restore
        ? {
            width: restore.size.width,
            height:
              restore.preferredHeight === compactHeight
                ? restore.size.height
                : preferredSize.height,
          }
        : preferredSize;
      await setBounds(windowBounds(monitor, size, restore));
    });
  }, [enqueue, expanded, compactHeight]);

  return returnToBottom;
}
