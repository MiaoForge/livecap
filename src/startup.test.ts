import { describe, expect, it } from "vitest";
import { loadStartupResources } from "./startup";

describe("startup resource loading", () => {
  it("retains successful data when another resource fails", async () => {
    const results = await loadStartupResources({
      settings: async () => {
        throw new Error("unreadable settings");
      },
      records: async () => [{ id: 1 }],
      usage: async () => [42],
    });
    expect(results.settings.status).toBe("rejected");
    expect(results.records).toEqual({
      status: "fulfilled",
      value: [{ id: 1 }],
    });
    expect(results.usage).toEqual({ status: "fulfilled", value: [42] });
  });

  it("isolates synchronous loader failures too", async () => {
    const results = await loadStartupResources({
      broken: () => {
        throw new Error("failed");
      },
      other: async () => true,
    });
    expect(results.broken.status).toBe("rejected");
    expect(results.other).toEqual({ status: "fulfilled", value: true });
  });
});
