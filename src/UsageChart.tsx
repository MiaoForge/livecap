import { money } from "./clientApi";

export type DailyUsage = { day: number; cost: number; minutes: number };
export function UsageChart({
  daily,
  metric,
  selected,
  onSelect,
}: {
  daily: DailyUsage[];
  metric: string;
  selected: number | null;
  onSelect: (day: number | null) => void;
}) {
  const max = Math.max(
    0.01,
    ...daily.map((day) => (metric === "cost" ? day.cost : day.minutes)),
  );
  return (
    <>
      <div
        className="usage-trend-scroll"
        role="region"
        aria-label="每日使用趋势"
        aria-describedby="usage-chart-hint"
        tabIndex={0}
      >
        <div
          className="bar-chart"
          style={{
            gridTemplateColumns: `repeat(${daily.length}, minmax(44px, 1fr))`,
            minWidth: daily.length * 44 + Math.max(0, daily.length - 1) * 8,
          }}
        >
          {daily.map((day, index) => {
            const label = `${new Date(day.day).toLocaleDateString("zh-CN")} · ${money(day.cost)} · ${day.minutes.toFixed(1)} 分钟`;
            return (
              <button
                key={day.day}
                title={label}
                aria-label={label}
                aria-pressed={selected === day.day}
                onClick={() => onSelect(selected === day.day ? null : day.day)}
                className={selected === day.day ? "selected" : ""}
              >
                <span className="bar-value">
                  {metric === "cost" ? money(day.cost) : day.minutes.toFixed(0)}
                </span>
                <span
                  className="chart-bar"
                  style={{
                    height:
                      ((metric === "cost" ? day.cost : day.minutes) / max) *
                        76 +
                      2 +
                      "%",
                  }}
                />
                <small>
                  {daily.length === 7 || index % 5 === 0
                    ? new Date(day.day).toLocaleDateString("zh-CN", {
                        month: "numeric",
                        day: "numeric",
                      })
                    : ""}
                </small>
              </button>
            );
          })}
        </div>
      </div>
      <p id="usage-chart-hint" className="usage-scroll-hint">
        可左右滚动查看日期，点击日期筛选明细。
      </p>
    </>
  );
}
