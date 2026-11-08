import { describe, expect, it } from "vitest";
import { followsAfterScroll } from "./scrollFollowing";

describe("transcript scroll following", () => {
  it("keeps following when new paragraphs increase the bottom gap", () => {
    expect(
      followsAfterScroll(
        true,
        { scrollTop: 400, scrollHeight: 900, clientHeight: 500 },
        {
          scrollTop: 400,
          scrollHeight: 1200,
          clientHeight: 500,
        },
      ),
    ).toBe(true);
    expect(
      followsAfterScroll(
        true,
        { scrollTop: 400, scrollHeight: 900, clientHeight: 500 },
        {
          scrollTop: 400,
          scrollHeight: 2400,
          clientHeight: 500,
        },
      ),
    ).toBe(true);
  });
  it("pauses on upward scrolling even when the reader is still near the bottom", () => {
    expect(
      followsAfterScroll(
        true,
        { scrollTop: 500, scrollHeight: 1000, clientHeight: 500 },
        {
          scrollTop: 490,
          scrollHeight: 1000,
          clientHeight: 500,
        },
      ),
    ).toBe(false);
    expect(
      followsAfterScroll(
        false,
        { scrollTop: 500, scrollHeight: 1000, clientHeight: 500 },
        {
          scrollTop: 495,
          scrollHeight: 1000,
          clientHeight: 500,
        },
      ),
    ).toBe(false);
  });
  it("does not resume a source jump just because its animation starts near the bottom", () => {
    let following = false;
    for (const [previous, top] of [
      [500, 495],
      [495, 480],
      [480, 200],
    ]) {
      following = followsAfterScroll(
        following,
        { scrollTop: previous, scrollHeight: 1000, clientHeight: 500 },
        {
          scrollTop: top,
          scrollHeight: 1000,
          clientHeight: 500,
        },
      );
      expect(following).toBe(false);
    }
  });
  it("keeps readers in place while new text arrives and resumes when they scroll down to the bottom", () => {
    expect(
      followsAfterScroll(
        false,
        { scrollTop: 200, scrollHeight: 1500, clientHeight: 500 },
        {
          scrollTop: 200,
          scrollHeight: 1500,
          clientHeight: 500,
        },
      ),
    ).toBe(false);
    expect(
      followsAfterScroll(
        false,
        { scrollTop: 200, scrollHeight: 1500, clientHeight: 500 },
        {
          scrollTop: 500,
          scrollHeight: 1500,
          clientHeight: 500,
        },
      ),
    ).toBe(false);
    expect(
      followsAfterScroll(
        false,
        { scrollTop: 500, scrollHeight: 1500, clientHeight: 500 },
        {
          scrollTop: 980,
          scrollHeight: 1500,
          clientHeight: 500,
        },
      ),
    ).toBe(true);
  });
  it("ignores stationary scroll notifications after a programmatic jump to latest", () => {
    expect(
      followsAfterScroll(
        true,
        { scrollTop: 1000, scrollHeight: 1500, clientHeight: 500 },
        {
          scrollTop: 1000,
          scrollHeight: 1500,
          clientHeight: 500,
        },
      ),
    ).toBe(true);
    expect(
      followsAfterScroll(
        false,
        { scrollTop: 1000, scrollHeight: 1500, clientHeight: 500 },
        {
          scrollTop: 1000,
          scrollHeight: 1500,
          clientHeight: 500,
        },
      ),
    ).toBe(false);
  });
  it("preserves following when a draft shrinks or the viewport grows", () => {
    expect(
      followsAfterScroll(
        true,
        { scrollTop: 500, scrollHeight: 1000, clientHeight: 500 },
        { scrollTop: 400, scrollHeight: 900, clientHeight: 500 },
      ),
    ).toBe(true);
    expect(
      followsAfterScroll(
        true,
        { scrollTop: 500, scrollHeight: 1000, clientHeight: 500 },
        { scrollTop: 300, scrollHeight: 1000, clientHeight: 700 },
      ),
    ).toBe(true);
    expect(
      followsAfterScroll(
        false,
        { scrollTop: 500, scrollHeight: 1000, clientHeight: 500 },
        { scrollTop: 300, scrollHeight: 1000, clientHeight: 700 },
      ),
    ).toBe(false);
  });
});
