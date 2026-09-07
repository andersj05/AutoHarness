import { expect, it } from "vitest";
import { fixtureMemoryRow } from "./fixtures";
import { emptyMemory } from "./model";
import { validateMemory } from "./validation";

it("rejects missing, duplicated, oversized, and imprecise memory baselines", () => {
  const page = { ...emptyMemory(), rows: [fixtureMemoryRow(1)], total: 1 };
  expect(() => validateMemory(page)).not.toThrow();
  expect(() => validateMemory(undefined as never)).toThrow("invalid memory page");
  expect(() => validateMemory({ ...page, rows: [...page.rows, ...page.rows], total: 2 })).toThrow();
  expect(() => validateMemory({ ...page, generation: "18446744073709551616" })).toThrow();
  const corrupt = structuredClone(page);
  corrupt.rows[0]!.detail!.revision_context!.expected_last_sequence = "1.5";
  expect(() => validateMemory(corrupt)).toThrow();
  corrupt.rows[0]!.detail!.content = "x".repeat(65_537);
  expect(() => validateMemory(corrupt)).toThrow();
});
