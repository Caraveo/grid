import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  effectiveScore,
  allocateProportional,
  splitEmission,
  resourceScore,
} from "./index.js";

describe("por scoring", () => {
  it("weights compute heavily", () => {
    const high = resourceScore({
      compute: 1,
      uptime: 0,
      fidelity: 0,
      efficiency: 0,
    });
    const low = resourceScore({
      compute: 0,
      uptime: 1,
      fidelity: 1,
      efficiency: 1,
    });
    assert.ok(high > 0.5);
    assert.ok(high > low * 0.5);
  });

  it("applies reputation multiplier", () => {
    const base = effectiveScore({
      compute: 1,
      uptime: 1,
      fidelity: 1,
      reputation: 1,
    });
    const boosted = effectiveScore({
      compute: 1,
      uptime: 1,
      fidelity: 1,
      reputation: 1.5,
    });
    assert.ok(boosted > base);
  });

  it("caps hyperscaler vs many peers at gamma=5%", () => {
    // 30 equal small clusters + 1 whale → effective gamma stays 5%
    const scores: Record<string, number> = { whale: 1000 };
    const clusterOf: Record<string, string> = { whale: "google" };
    for (let i = 0; i < 30; i++) {
      scores[`h${i}`] = 1;
      clusterOf[`h${i}`] = `home${i}`;
    }
    const rewards = allocateProportional(scores, 100, clusterOf, 0.05);
    assert.ok((rewards.whale ?? 0) <= 5 + 1e-6, `whale got ${rewards.whale}`);
    const homes = Object.keys(scores)
      .filter((k) => k !== "whale")
      .reduce((s, k) => s + (rewards[k] ?? 0), 0);
    assert.ok(homes >= 94, `homes got ${homes}`);
  });

  it("still distributes full pool when few clusters (dynamic gamma floor)", () => {
    const scores = { a: 100, b: 1, c: 1 };
    const clusterOf = { a: "google", b: "home1", c: "home2" };
    const rewards = allocateProportional(scores, 100, clusterOf, 0.05);
    const sum = Object.values(rewards).reduce((a, b) => a + b, 0);
    assert.ok(Math.abs(sum - 100) < 1e-6, `sum ${sum}`);
    // equal-split floor = 1/3; whale cannot take pure proportional ~98%
    assert.ok((rewards.a ?? 0) <= 100 / 3 + 1e-6);
  });

  it("splits emission 90/10", () => {
    const { proportional, inclusion } = splitEmission(1000);
    assert.equal(proportional, 900);
    assert.equal(inclusion, 100);
  });
});
