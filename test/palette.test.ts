import { describe, expect, test } from "vitest";
import { buildPal32 } from "../src/utils/palette";

// Simple palette conversion tests to ensure precomputed lookups behave correctly

describe("buildPal32", () => {
  test("converts RGB palette to Uint32 RGBA and BGRA with transparency", () => {
    // Two colors: red and green
    const raw = new Uint8Array([
      255, 0, 0, // red
      0, 255, 0, // green
    ]);
    const rgba = buildPal32(raw, 0, 2, "rgba", 1);
    const bgra = buildPal32(raw, 0, 2, "bgra", 1);

    // Red with full alpha in RGBA order
    expect(rgba[0]).toBe(0xff0000ff);
    // Green is transparent (alpha 0)
    expect(rgba[1]).toBe(0x0000ff00);

    // Red with full alpha in BGRA order
    expect(bgra[0]).toBe(0xffff0000);
    // Green transparent again
    expect(bgra[1]).toBe(0x0000ff00);
  });
});
