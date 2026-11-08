export type ScrollPosition = {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
};

export function scrollPosition(element: ScrollPosition): ScrollPosition {
  return {
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  };
}

export function followsAfterScroll(
  following: boolean,
  previous: ScrollPosition,
  position: ScrollPosition,
): boolean {
  // Layout changes, new text and revised drafts can move the scrollbar without
  // the reader scrolling. Preserve their intent through those changes.
  if (
    previous.scrollHeight !== position.scrollHeight ||
    previous.clientHeight !== position.clientHeight
  )
    return following;
  const movement = position.scrollTop - previous.scrollTop;
  if (movement < -1) return false;
  if (
    movement > 1 &&
    position.scrollHeight - position.clientHeight - position.scrollTop <= 48
  )
    return true;
  return following;
}
