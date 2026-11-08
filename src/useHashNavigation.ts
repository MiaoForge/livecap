import { useEffect, useRef, useState } from "react";
import { navigateHash, observeHash, pageFromHash } from "./hashNavigation";

export function useHashNavigation(onHistoryChange: () => void) {
  const [page, setPage] = useState(() => pageFromHash(window.location.hash));
  const historyChange = useRef(onHistoryChange);
  historyChange.current = onHistoryChange;
  useEffect(
    () =>
      observeHash(window, (next) => {
        setPage(next);
        historyChange.current();
      }),
    [],
  );
  return {
    page,
    navigate: (next: string) => setPage(navigateHash(window, next)),
  };
}
