# Livecap desktop interaction prototype

Open http://127.0.0.1:4317/desktop/ with the static server rooted at `prototypes/livecap-studio`.

This proposal replaces the web console with three desktop surfaces:
- A 348 px control window: start, pause, stop, domain and session cost.
- A bottom caption strip: drag its header to float; use its dock button or double-click the header to dock again. Tools appear on hover or keyboard focus.
- A separate movable settings window: caption appearance, languages, domain preparation and service connection.

Starting hides the control window. The taskbar and tray buttons reopen it. Closing settings leaves captions running. Ending returns to the control window. Space pauses captions when no form control or settings window has focus; Escape closes settings.

This is a browser interaction study, not native window integration. Background video, transcript, progress, service status and cost are sample data. No audio capture, credentials or cloud calls occur. Knowledge examples persist in browser local storage and can be exported as JSON; actual app file caching and Windows docking remain integration work after design approval.

Animation uses requestAnimationFrame and incremental text updates, with a fixed caption text area to reduce layout movement. Reduced-motion preferences disable decorative animation.
