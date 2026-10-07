// Drives core.js with the simulator's known motions and checks it recovers them.
// Run: node tools/imu_web/test_core.mjs
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const { Engine } = createRequire(import.meta.url)('./static/core.js');

const samples = JSON.parse(execFileSync('python3', [path.join(here, 'sources.py'), 'dump-sim', '60'], { maxBuffer: 1 << 28 }));

const expected = [ // label, axis, signed degrees, gravity rotation expected (deg)
  ['yaw-left', 'y', -45, 0], ['yaw-right', 'y', 45, 0],
  ['pitch-up', 'x', 30, 30], ['pitch-down', 'x', -30, 30],
  ['roll-left', 'z', 30, 30], ['roll-right', 'z', -30, 30],
];

const eng = new Engine();
const results = [];
let calib = null;
eng.on('calibration', (c) => { calib = c; eng.beginPhase('go'); });
eng.on('phase', (r) => {
  results.push(r);
  if (results.length < expected.length * 2) eng.beginPhase(results.length % 2 ? 'return' : 'go');
});

eng.startCalibration(3);
for (let i = 0; i < samples.length; i += 40) eng.feed(samples.slice(i, i + 40));

assert.ok(calib && calib.ok, 'calibration should succeed on still data');
assert.ok(Math.abs(calib.bias[0] - 0.012) < 0.002 && Math.abs(calib.bias[1] + 0.009) < 0.002, 'recovers the simulated bias');
assert.ok(Math.abs(calib.gravityMag - 9.77) < 0.05, 'gravity magnitude');
assert.equal(results.length, expected.length * 2, `expected 12 phases, got ${results.length}`);

expected.forEach(([label, axis, deg, grav], k) => {
  const go = results[2 * k], back = results[2 * k + 1];
  assert.equal(go.dominant, axis, `${label}: dominant axis`);
  assert.ok(Math.abs(go.deltaDominant - deg) < 2, `${label}: go angle ${go.deltaDominant.toFixed(1)} vs ${deg}`);
  assert.equal(go.sign, Math.sign(deg), `${label}: sign`);
  assert.ok(Math.abs(go.gravityRotDeg - grav) < 3, `${label}: gravity rotation ${go.gravityRotDeg.toFixed(1)} vs ${grav}`);
  assert.ok(go.mixedRatio < 0.1, `${label}: stays on one axis`);
  assert.ok(Math.abs(go.deltaDominant + back.deltaDominant) < 1, `${label}: returns to start (closure ${(go.deltaDominant + back.deltaDominant).toFixed(2)}°)`);
  console.log(`ok  ${label.padEnd(11)} ${go.dominant}${go.sign > 0 ? '+' : '-'} ${go.deltaDominant.toFixed(1).padStart(6)}°  gravity ${go.gravityRotDeg.toFixed(1).padStart(5)}°  closure ${(go.deltaDominant + back.deltaDominant).toFixed(2)}°`);
});

// A twitch must not complete a phase.
const e2 = new Engine();
let fired = 0, ignored = 0;
e2.on('phase', () => fired++).on('ignored', () => ignored++);
e2.startCalibration(3);
e2.feed(samples.slice(0, 3500));
e2.beginPhase('twitch');
const base = samples[3499];
for (let i = 1; i <= 3000; i++) {
  const t = base[0] + i / 1000;
  const burst = i > 500 && i < 600 ? 0.3 : 0;   // 0.1 s at 0.3 rad/s = ~1.7 deg
  e2.feed([[t, 0.012, -0.009 + burst, 0.007, 0, -9.77, 0]]);
}
assert.equal(fired, 0, 'twitch must not finish a phase');
assert.ok(ignored >= 1, 'twitch should be reported as ignored');
console.log('ok  twitch ignored');
console.log('all core tests passed');
