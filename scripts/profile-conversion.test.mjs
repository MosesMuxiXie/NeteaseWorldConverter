import test from 'node:test';
import assert from 'node:assert/strict';
import { timingFromLines } from './profile-conversion.mjs';

test('extracts only numeric performance timings from application logs', () => {
  const lines = [
    '2026-09-23 [INFO] PERF key_recovery_ms=110',
    '2026-09-23 [INFO] NWC_PHASE init 1250',
    '2026-09-23 [INFO] recovered key: secret',
    '2026-09-23 [INFO] PERF b2j_ms=235799',
  ];
  assert.deepEqual(timingFromLines(lines), {
    key_recovery_ms: 110,
    'b2j.init_ms': 1250,
    b2j_ms: 235799,
  });
});
