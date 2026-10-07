/* Pure IMU logic for the viewer: bias calibration, gyro integration and "move, then hold" detection.
 * No DOM access, so it runs in the browser and in Node (see test_core.mjs).
 * Samples are [t, gx, gy, gz, ax, ay, az] with t in seconds, gyro in rad/s, accel in m/s^2. */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.XrealCore = factory();
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const RAD2DEG = 180 / Math.PI;
  const AXES = ['x', 'y', 'z'];
  const DEFAULTS = {
    moveOn: 0.12,        // smoothed |gyro| (rad/s) above which we call it moving
    holdBelow: 0.06,     // ...and below which we call it still
    holdTime: 1.0,       // seconds of stillness that end a phase
    minPathDeg: 8,       // ignore twitches that rotate less than this in total
    smoothTau: 0.1,      // seconds, smoothing of |gyro|
    accelTau: 0.2,       // seconds, smoothing of accel (gravity direction)
    calibStdMax: 0.02,   // rad/s, calibration is rejected if the gyro wobbles more than this
    calibMeanMax: 0.1,   // rad/s, ...or if the bias is implausibly large
    gapMax: 0.05,        // seconds, a longer gap between samples is not integrated across
  };

  const norm = (v) => Math.hypot(v[0], v[1], v[2]);

  function gravityRotation(a, b) {
    const na = norm(a), nb = norm(b);
    if (!na || !nb) return { deg: NaN, axis: [0, 0, 0] };
    const dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (na * nb);
    const c = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    const nc = norm(c) || 1;
    return { deg: Math.acos(Math.max(-1, Math.min(1, dot))) * RAD2DEG, axis: c.map((x) => x / nc) };
  }

  class Engine {
    constructor(opts) {
      this.o = Object.assign({}, DEFAULTS, opts);
      this.handlers = {};
      this.onSample = null;   // (t, gyroCorrected[3], accel[3], anglesDeg[3]) after every sample
      this.bias = [0, 0, 0];
      this.calibrated = false;
      this.ang = [0, 0, 0];
      this.calib = null;
      this.phase = null;
      this.resetStream();
    }

    on(name, fn) { this.handlers[name] = fn; return this; }
    emit(name, payload) { if (this.handlers[name]) this.handlers[name](payload); }

    /** The source restarted its clock (reconnect or replay loop). Keeps bias and angles. */
    resetStream() {
      this.lastT = null;
      this.sm = null;
      this.al = null;
    }

    zeroAngles() { this.ang = [0, 0, 0]; }

    startCalibration(seconds = 3) {
      this.calib = { dur: seconds, t0: null, n: 0, sg: [0, 0, 0], sg2: [0, 0, 0], sa: [0, 0, 0], progress: 0 };
    }

    beginPhase(label) {
      this.phase = {
        label, state: 'waiting', startAng: this.ang.slice(), startAccel: (this.al || [0, 0, 0]).slice(),
        pathDeg: 0, peak: 0, motionStart: null, holdStart: null, poseAng: null,
      };
    }

    cancelPhase() { this.phase = null; }

    feed(batch) { for (const s of batch) this._process(s[0], [s[1], s[2], s[3]], [s[4], s[5], s[6]]); }

    snapshot() {
      return {
        angles: this.ang.slice(), bias: this.bias.slice(), calibrated: this.calibrated,
        smoothed: this.sm || 0, phaseState: this.phase ? this.phase.state : null,
        calibProgress: this.calib ? this.calib.progress : null,
      };
    }

    _process(t, g, a) {
      let dt = this.lastT === null ? 0 : t - this.lastT;
      if (dt < 0) dt = 0;
      if (dt > this.o.gapMax) dt = 0.001;
      this.lastT = t;

      if (this.calib) this._calibrate(t, g, a);

      const gc = [g[0] - this.bias[0], g[1] - this.bias[1], g[2] - this.bias[2]];
      for (let i = 0; i < 3; i++) this.ang[i] += gc[i] * dt * RAD2DEG;

      const mag = norm(gc);
      this.sm = this.sm === null ? mag : this.sm + (1 - Math.exp(-dt / this.o.smoothTau)) * (mag - this.sm);
      if (this.al === null) this.al = a.slice();
      else {
        const k = 1 - Math.exp(-dt / this.o.accelTau);
        for (let i = 0; i < 3; i++) this.al[i] += k * (a[i] - this.al[i]);
      }

      if (this.phase && this.phase.state !== 'done') this._phase(t, gc, mag, dt);
      if (this.onSample) this.onSample(t, gc, a, this.ang);
    }

    _calibrate(t, g, a) {
      const c = this.calib;
      if (c.t0 === null) c.t0 = t;
      c.n++;
      for (let i = 0; i < 3; i++) { c.sg[i] += g[i]; c.sg2[i] += g[i] * g[i]; c.sa[i] += a[i]; }
      c.progress = Math.min(1, (t - c.t0) / c.dur);
      if (t - c.t0 < c.dur) return;
      const mean = c.sg.map((s) => s / c.n);
      const std = c.sg2.map((s, i) => Math.sqrt(Math.max(0, s / c.n - mean[i] * mean[i])));
      const gravity = c.sa.map((s) => s / c.n);
      const ok = Math.max(...std) <= this.o.calibStdMax && Math.max(...mean.map(Math.abs)) <= this.o.calibMeanMax;
      const res = { ok, bias: mean, std, gravity, gravityMag: norm(gravity), samples: c.n };
      this.calib = null;
      if (ok) { this.bias = mean; this.calibrated = true; this.zeroAngles(); }
      this.emit('calibration', res);
    }

    _phase(t, gc, mag, dt) {
      const p = this.phase, o = this.o;
      p.pathDeg += mag * dt * RAD2DEG;
      p.peak = Math.max(p.peak, mag);
      if (p.state === 'waiting') {
        if (this.sm > o.moveOn) { p.state = 'moving'; p.motionStart = t; }
      } else if (p.state === 'moving') {
        if (this.sm < o.holdBelow) { p.state = 'holding'; p.holdStart = t; p.poseAng = this.ang.slice(); }
      } else if (p.state === 'holding') {
        if (this.sm > o.holdBelow * 2) { p.state = 'moving'; p.poseAng = null; }
        else if (t - p.holdStart >= o.holdTime) this._finish(t);
      }
    }

    _finish(t) {
      const p = this.phase;
      if (p.pathDeg < this.o.minPathDeg) {   // a twitch, not a move: keep waiting
        p.state = 'waiting'; p.pathDeg = 0; p.peak = 0; p.startAng = p.poseAng.slice();
        this.emit('ignored', { label: p.label, reason: 'movement too small' });
        return;
      }
      const delta = p.poseAng.map((v, i) => v - p.startAng[i]);
      let dom = 0;
      for (let i = 1; i < 3; i++) if (Math.abs(delta[i]) > Math.abs(delta[dom])) dom = i;
      const others = delta.filter((_, i) => i !== dom).map(Math.abs);
      const grav = gravityRotation(p.startAccel, this.al);
      p.state = 'done';
      this.emit('phase', {
        label: p.label, delta, dominant: AXES[dom], dominantIndex: dom, sign: Math.sign(delta[dom]),
        deltaDominant: delta[dom], mixedRatio: Math.max(...others) / (Math.abs(delta[dom]) || 1),
        peakRate: p.peak, pathDeg: p.pathDeg, seconds: p.holdStart - p.motionStart,
        gravityRotDeg: grav.deg, gravityAxis: grav.axis,
      });
    }
  }

  return { Engine, gravityRotation, AXES, RAD2DEG, DEFAULTS };
});
