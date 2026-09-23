// Repeatable Windows b2j comparison for the prepared Bedrock world.
// node scripts/bench-b2j.mjs <bedrock-dir> <original-b2j.exe> <candidate-b2j.exe|-> <validator.exe> <new-results-dir> [rounds=4]
import { spawn } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { performance } from 'node:perf_hooks';
import { pathToFileURL } from 'node:url';

const configurations = [
  { backend: 'original', threads: 8 },
  { backend: 'candidate', threads: 8 },
  { backend: 'original', threads: 12 },
  { backend: 'candidate', threads: 12 },
];
const latinOrder = [
  [0, 1, 2, 3],
  [1, 3, 0, 2],
  [2, 0, 3, 1],
  [3, 2, 1, 0],
];

export function makeSchedule(rounds = 4, includeCandidate = true) {
  if (!Number.isInteger(rounds) || rounds < 1) throw new Error('rounds must be a positive integer');
  if (!includeCandidate) {
    return Array.from({ length: rounds }, (_, round) =>
      (round % 2 === 0 ? [0, 2] : [2, 0]).map((index, position) => ({
        round: round + 1,
        position,
        ...configurations[index],
      })),
    ).flat();
  }
  return Array.from({ length: rounds }, (_, round) =>
    latinOrder[round % latinOrder.length].map((index, position) => ({
      round: round + 1,
      position,
      ...configurations[index],
    })),
  ).flat();
}

export function parseBackendOutput(output) {
  const phasesMs = {};
  for (const match of output.matchAll(/^NWC_PHASE (init|convert|terraform) (\d+)\s*$/gm)) {
    phasesMs[match[1]] = Number(match[2]);
  }
  const convertedChunks = Number(output.match(/^(\d+) chunks converted\s*$/m)?.[1] ?? 0);
  const terraformedChunks = Number(output.match(/^(\d+) chunks terraformed\s*$/m)?.[1] ?? 0);
  return { phasesMs, convertedChunks, terraformedChunks };
}

export function checkValidation(validation) {
  if (validation.region_files !== 190 || validation.region_chunks !== 50382) {
    throw new Error('unexpected region totals');
  }
  if (validation.level_version !== '1.21.10' || validation.data_version !== 4556) {
    throw new Error('unexpected world data version');
  }
}

function run(program, args, logPath) {
  return new Promise((resolvePromise, reject) => {
    const output = createWriteStream(logPath, { flags: 'wx' });
    const child = spawn(program, args, { windowsHide: true });
    let text = '';
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, 40 * 60 * 1000);
    for (const stream of [child.stdout, child.stderr]) {
      stream.on('data', (chunk) => {
        output.write(chunk);
        text += chunk.toString();
      });
    }
    child.on('error', (error) => {
      clearTimeout(timeout);
      output.end();
      reject(error);
    });
    child.on('close', (code) => {
      clearTimeout(timeout);
      output.end();
      if (timedOut) reject(new Error(`${program} exceeded 40 minutes`));
      else if (code !== 0) reject(new Error(`${program} exited ${code}; see ${logPath}`));
      else resolvePromise(text);
    });
  });
}

function median(values) {
  const sorted = values.toSorted((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

export function compareMedians(summary) {
  return summary.filter((entry) => entry.backend === 'original').map((original) => {
    const candidate = summary.find((entry) => entry.backend === 'candidate' && entry.threads === original.threads);
    if (!candidate) throw new Error(`missing candidate results for ${original.threads} threads`);
    return {
      threads: original.threads,
      improvementPercent: ((original.medianSeconds - candidate.medianSeconds) / original.medianSeconds) * 100,
      qualifies: candidate.medianSeconds <= original.medianSeconds * 0.9,
    };
  });
}

export function replacementDecision(summary) {
  const original = summary.filter((entry) => entry.backend === 'original').toSorted((a, b) => a.medianSeconds - b.medianSeconds)[0];
  const candidate = summary.filter((entry) => entry.backend === 'candidate').toSorted((a, b) => a.medianSeconds - b.medianSeconds)[0];
  if (!original || !candidate) throw new Error('both binaries need validated benchmark results');
  return {
    originalThreads: original.threads,
    candidateThreads: candidate.threads,
    improvementPercent: ((original.medianSeconds - candidate.medianSeconds) / original.medianSeconds) * 100,
    eligibleForReplacement: candidate.medianSeconds <= original.medianSeconds * 0.9,
  };
}

async function main() {
  const [worldArg, originalArg, candidateArg, validatorArg, resultsArg, roundsArg] = process.argv.slice(2);
  if (!worldArg || !originalArg || !candidateArg || !validatorArg || !resultsArg) {
    throw new Error('usage: node scripts/bench-b2j.mjs <bedrock-dir> <original-b2j.exe> <candidate-b2j.exe|-> <validator.exe> <new-results-dir> [rounds=4]');
  }
  const world = resolve(worldArg);
  const original = resolve(originalArg);
  const candidate = candidateArg === '-' ? null : resolve(candidateArg);
  const validator = resolve(validatorArg);
  const resultsDir = resolve(resultsArg);
  for (const file of [join(world, 'level.dat'), original, candidate, validator].filter(Boolean)) {
    if (!existsSync(file)) throw new Error(`missing input: ${file}`);
  }
  if (existsSync(resultsDir)) throw new Error(`results directory already exists: ${resultsDir}`);
  mkdirSync(resultsDir);
  const schedule = makeSchedule(roundsArg === undefined ? 4 : Number(roundsArg), candidate !== null);
  const results = [];
  for (let index = 0; index < schedule.length; index++) {
    const entry = schedule[index];
    const program = entry.backend === 'original' ? original : candidate;
    const name = `${String(index + 1).padStart(2, '0')}-${entry.backend}-${entry.threads}`;
    const output = join(resultsDir, `${name}-world`);
    mkdirSync(output);
    const start = performance.now();
    const stdout = await run(program, ['-i', world, '-o', output, '-n', String(entry.threads)], join(resultsDir, `${name}.log`));
    const seconds = (performance.now() - start) / 1000;
    const parsed = parseBackendOutput(stdout);
    if (parsed.convertedChunks !== 25057 || parsed.terraformedChunks !== 25057) {
      throw new Error(`${name}: unexpected b2j chunk totals`);
    }
    const validationText = await run(validator, ['validate', output], join(resultsDir, `${name}-validation.log`));
    const validation = JSON.parse(validationText).validation;
    try {
      checkValidation(validation);
    } catch (error) {
      throw new Error(`${name}: ${error.message}`);
    }
    results.push({ ...entry, seconds, ...parsed, validation });
    writeFileSync(join(resultsDir, 'results.json'), JSON.stringify({ world, original, candidate, results }, null, 2));
    process.stdout.write(`${name}: ${seconds.toFixed(3)} s\n`);
  }
  const summary = configurations.filter((configuration) => candidate !== null || configuration.backend === 'original').map((configuration) => {
    const times = results
      .filter((result) => result.backend === configuration.backend && result.threads === configuration.threads)
      .map((result) => result.seconds);
    return { ...configuration, medianSeconds: median(times), times };
  });
  const comparison = candidate === null ? [] : compareMedians(summary);
  const replacement = candidate === null ? null : replacementDecision(summary);
  const report = {
    configurations: summary,
    comparison,
    replacement,
  };
  writeFileSync(join(resultsDir, 'summary.json'), JSON.stringify(report, null, 2));
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
