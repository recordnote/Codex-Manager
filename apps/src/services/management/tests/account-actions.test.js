import test from "node:test";
import assert from "node:assert/strict";

import { __accountImportTestHooks } from "../account-actions.js";

test("chunkItems splits import payload into fixed-size batches", () => {
  const input = Array.from({ length: 123 }, (_, index) => index + 1);
  const batches = __accountImportTestHooks.chunkItems(input, 50);

  assert.equal(batches.length, 3);
  assert.equal(batches[0].length, 50);
  assert.equal(batches[1].length, 50);
  assert.equal(batches[2].length, 23);
});

test("importContentsInParallel aggregates batched import results", async () => {
  const contents = Array.from({ length: 120 }, (_, index) => `account-${index + 1}`);
  const seenBatchSizes = [];

  const summary = await __accountImportTestHooks.importContentsInParallel(
    contents,
    async (batch) => {
      seenBatchSizes.push(batch.length);
      return {
        total: batch.length,
        created: batch.length - 1,
        updated: 1,
        failed: 0,
        errors: [],
      };
    },
  );

  assert.deepEqual(seenBatchSizes, [50, 50, 20]);
  assert.equal(summary.total, 120);
  assert.equal(summary.created, 117);
  assert.equal(summary.updated, 3);
  assert.equal(summary.failed, 0);
  assert.deepEqual(summary.errors, []);
});

test("importContentsInParallel records failed batch as failed items", async () => {
  const contents = Array.from({ length: 55 }, (_, index) => `account-${index + 1}`);

  const summary = await __accountImportTestHooks.importContentsInParallel(
    contents,
    async (batch) => {
      if (batch[0] === "account-51") {
        throw new Error("batch failed");
      }
      return {
        total: batch.length,
        created: batch.length,
        updated: 0,
        failed: 0,
        errors: [],
      };
    },
  );

  assert.equal(summary.total, 55);
  assert.equal(summary.created, 50);
  assert.equal(summary.updated, 0);
  assert.equal(summary.failed, 5);
  assert.equal(summary.errors.length, 1);
  assert.equal(summary.errors[0].index, 51);
  assert.match(summary.errors[0].message, /batch failed/);
});
