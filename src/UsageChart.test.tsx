import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { UsageChart } from "./UsageChart";

it.each([7, 30])(
  "keeps %i daily targets readable inside a labelled scroll region",
  (days) => {
    const daily = Array.from({ length: days }, (_, index) => ({
      day: Date.UTC(2026, 10, index + 1),
      cost: index,
      minutes: index * 2,
    }));
    const html = renderToStaticMarkup(
      <UsageChart
        daily={daily}
        metric="cost"
        selected={daily[1].day}
        onSelect={() => {}}
      />,
    );
    expect(html).toContain('class="usage-trend-scroll" role="region"');
    expect(html).toContain('tabindex="0"');
    expect(html).toContain(`repeat(${days}, minmax(44px, 1fr))`);
    expect(html.match(/<button /g)).toHaveLength(days);
    expect(html.match(/aria-pressed="true"/g)).toHaveLength(1);
    expect(html.match(/aria-pressed="false"/g)).toHaveLength(days - 1);
    expect(html).toContain("可左右滚动查看日期");
    expect(html).toContain('aria-describedby="usage-chart-hint"');
  },
);
