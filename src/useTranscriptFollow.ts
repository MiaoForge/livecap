import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, TouchEvent, UIEvent, WheelEvent } from "react";
import { followsAfterScroll, scrollPosition } from "./scrollFollowing";

export function useTranscriptFollow(content: unknown, visible = true) {
  const pane = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  const followingRef = useRef(true);
  const previousPosition = useRef({
    scrollTop: 0,
    scrollHeight: 0,
    clientHeight: 0,
  });
  const touchY = useRef<number | null>(null);

  function syncToBottom() {
    const element = pane.current;
    if (!visible || !element || element.clientHeight === 0) return;
    element.scrollTop = element.scrollHeight;
    previousPosition.current = scrollPosition(element);
  }
  function changeFollowing(next: boolean) {
    followingRef.current = next;
    setFollowing(next);
  }
  useLayoutEffect(() => {
    if (followingRef.current) syncToBottom();
  }, [content, visible, following]);
  useEffect(() => {
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (followingRef.current) syncToBottom();
    });
    if (pane.current) observer.observe(pane.current);
    if (body.current) observer.observe(body.current);
    return () => observer.disconnect();
  }, [visible]);

  function onScroll(event: UIEvent<HTMLDivElement>) {
    const element = event.currentTarget;
    // Hidden tabs can reset their geometry; that is not a request to stop following.
    if (!visible || element.clientHeight === 0) return;
    const next = followsAfterScroll(
      followingRef.current,
      previousPosition.current,
      element,
    );
    const layoutChanged =
      previousPosition.current.scrollHeight !== element.scrollHeight ||
      previousPosition.current.clientHeight !== element.clientHeight;
    previousPosition.current = scrollPosition(element);
    if (next && layoutChanged) syncToBottom();
    if (next !== followingRef.current) changeFollowing(next);
  }
  function pauseForReader() {
    if (pane.current) previousPosition.current = scrollPosition(pane.current);
    changeFollowing(false);
  }
  function onWheel(event: WheelEvent<HTMLDivElement>) {
    if (event.deltaY < 0 && event.currentTarget.scrollTop > 0) pauseForReader();
  }
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (
      ["ArrowUp", "PageUp", "Home"].includes(event.key) &&
      event.currentTarget.scrollTop > 0
    )
      pauseForReader();
  }
  function onTouchStart(event: TouchEvent<HTMLDivElement>) {
    touchY.current = event.touches[0]?.clientY ?? null;
  }
  function onTouchMove(event: TouchEvent<HTMLDivElement>) {
    const next = event.touches[0]?.clientY;
    if (
      next != null &&
      touchY.current != null &&
      next > touchY.current &&
      event.currentTarget.scrollTop > 0
    )
      pauseForReader();
    touchY.current = next ?? null;
  }
  return {
    pane,
    body,
    following,
    onScroll,
    onWheel,
    onKeyDown,
    onTouchStart,
    onTouchMove,
    pause: pauseForReader,
    resume: () => {
      changeFollowing(true);
      syncToBottom();
    },
  };
}
