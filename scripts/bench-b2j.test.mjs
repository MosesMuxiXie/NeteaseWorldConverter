import test from 'node:test';
import assert from 'node:assert/strict';
import { checkValidation, compareMedians, makeSchedule, parseBackendOutput, replacementDecision } from './bench-b2j.mjs';

test('benchmark order gives each configuration every position once', () => {
  const schedule = makeSchedule(4);
  assert.equal(schedule.length, 16);
  const configurations = new Set(schedule.map((run) => `${run.backend}/${run.threads}`));
  assert.equal(configurations.size, 4);
  for (const configuration of configurations) {
    const positions = schedule
      .filter((run) => `${run.backend}/${run.threads}` === configuration)
      .map((run) => run.position)
      .sort();
    assert.deepEqual(positions, [0, 1, 2, 3]);
  }
});

test('thread-only schedule alternates order without a candidate binary', () => {
  const schedule = makeSchedule(4, false);
  assert.equal(schedule.length, 8);
  assert.deepEqual(schedule.map((run) => run.threads), [8, 12, 12, 8, 8, 12, 12, 8]);
  assert.ok(schedule.every((run) => run.backend === 'original'));
});

test('parses phase timings and converted chunk totals', () => {
  const output = [
    'NWC_PHASE init 1250',
    'NWC_PHASE convert 190000',
    'NWC_PHASE terraform 45000',
    '25057 chunks converted',
    '25057 chunks terraformed',
  ].join('\n');
  assert.deepEqual(parseBackendOutput(output), {
    phasesMs: { init: 1250, convert: 190000, terraform: 45000 },
    convertedChunks: 25057,
    terraformedChunks: 25057,
  });
});

test('requires at least ten percent median b2j improvement before binary replacement', () => {
  const summary = [
    { backend: 'original', threads: 8, medianSeconds: 400 },
    { backend: 'candidate', threads: 8, medianSeconds: 362 },
    { backend: 'original', threads: 12, medianSeconds: 300 },
    { backend: 'candidate', threads: 12, medianSeconds: 270 },
  ];
  assert.deepEqual(compareMedians(summary), [
    { threads: 8, improvementPercent: 9.5, qualifies: false },
    { threads: 12, improvementPercent: 10, qualifies: true },
  ]);
});

test('replacement compares the best validated thread setting for each binary', () => {
  const summary = [
    { backend: 'original', threads: 8, medianSeconds: 200 },
    { backend: 'candidate', threads: 8, medianSeconds: 180 },
    { backend: 'original', threads: 12, medianSeconds: 300 },
    { backend: 'candidate', threads: 12, medianSeconds: 250 },
  ];
  assert.deepEqual(replacementDecision(summary), {
    originalThreads: 8,
    candidateThreads: 8,
    improvementPercent: 10,
    eligibleForReplacement: true,
  });
  summary[1].medianSeconds = 190;
  assert.equal(replacementDecision(summary).eligibleForReplacement, false);
});

test('rejects a world with correct region counts but the wrong target version', () => {
  const validation = { region_files: 190, region_chunks: 50382, level_version: '1.21.10', data_version: 4556 };
  assert.doesNotThrow(() => checkValidation(validation));
  assert.throws(() => checkValidation({ ...validation, level_version: '1.20.4' }), /world data/);
});
