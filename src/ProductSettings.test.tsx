import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { ProductSettings } from "./ProductSettings";
import { defaults } from "./clientApi";

it.each([
  { microphone: true, active: false },
  { microphone: false, active: false },
  { microphone: true, active: true },
])(
  "shows the saved meeting microphone setting (%j)",
  ({ microphone, active }) => {
    const html = renderToStaticMarkup(
      <ProductSettings
        settings={{ ...defaults, meetingMicrophone: microphone }}
        setSettings={() => {}}
        keys={{}}
        onKeys={() => {}}
        active={active}
        preview={() => {}}
        fail={() => {}}
      />,
    );
    const toggle = html.match(/<button[^>]*aria-label="会议麦克风"[^>]*>/)?.[0];
    expect(toggle).toContain(`aria-checked="${microphone}"`);
    expect(toggle?.includes('disabled=""')).toBe(active);
  },
);
