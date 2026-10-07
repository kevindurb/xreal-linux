/* UI for the XREAL IMU check page: live charts, calibration, guided direction tests. */
(function () {
  'use strict';
  const { Engine, AXES } = window.XrealCore;
  const $ = (id) => document.getElementById(id);
  const fmt = (v, d = 1) => (v >= 0 ? '+' : '−') + Math.abs(v).toFixed(d);

  // ---- ring buffers: 0-2 gyro, 3-5 accel, 6-8 integrated angle ----------------------------------
  const CAP = 12000;
  const buf = { t: new Float64Array(CAP), ch: Array.from({ length: 9 }, () => new Float32Array(CAP)), head: 0, count: 0 };
  const push = (t, vals) => {
    buf.t[buf.head] = t;
    for (let i = 0; i < 9; i++) buf.ch[i][buf.head] = vals[i];
    buf.head = (buf.head + 1) % CAP;
    buf.count = Math.min(CAP, buf.count + 1);
  };
  const at = (k) => (buf.head - buf.count + k + CAP) % CAP;   // k-th oldest sample
  const clearBuf = () => { buf.head = 0; buf.count = 0; };

  const engine = new Engine();
  engine.onSample = (t, g, a, ang) => push(t, [g[0], g[1], g[2], a[0], a[1], a[2], ang[0], ang[1], ang[2]]);

  // ---- charts -----------------------------------------------------------------------------------
  const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const charts = [
    { el: $('c-gyro'), first: 0, symmetric: true, minSpan: 0.4, unit: 'rad/s' },
    { el: $('c-accel'), first: 3, symmetric: false, minSpan: 4, unit: 'm/s²' },
    { el: $('c-ang'), first: 6, symmetric: true, minSpan: 20, unit: '°' },
  ];
  const WINDOW_S = 10;

  function drawChart(c) {
    const canvas = c.el, dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    if (buf.count < 2) return;

    const tEnd = buf.t[at(buf.count - 1)], tStart = tEnd - WINDOW_S;
    let lo = 0, hi = buf.count - 1;                       // first index with t >= tStart
    while (lo < hi) { const mid = (lo + hi) >> 1; if (buf.t[at(mid)] < tStart) lo = mid + 1; else hi = mid; }
    const i0 = lo, n = buf.count - i0;
    if (n < 2) return;

    let min = Infinity, max = -Infinity;
    for (let s = 0; s < 3; s++) {
      const arr = buf.ch[c.first + s];
      for (let k = i0; k < buf.count; k++) { const v = arr[at(k)]; if (v < min) min = v; if (v > max) max = v; }
    }
    if (c.symmetric) { const m = Math.max(c.minSpan / 2, Math.abs(min), Math.abs(max)) * 1.1; min = -m; max = m; }
    else { const mid = (min + max) / 2, half = Math.max(c.minSpan / 2, (max - min) / 2 * 1.1); min = mid - half; max = mid + half; }

    const left = 46, right = 6, top = 6, bottom = 6;
    const pw = w - left - right, ph = h - top - bottom;
    const Y = (v) => top + ph - ((v - min) / (max - min)) * ph;

    ctx.strokeStyle = css('--border'); ctx.fillStyle = css('--muted'); ctx.lineWidth = 1;
    ctx.font = '11px system-ui, sans-serif'; ctx.textAlign = 'right';
    for (const v of [min, (min + max) / 2, max]) {
      ctx.beginPath(); ctx.moveTo(left, Y(v) + 0.5); ctx.lineTo(w - right, Y(v) + 0.5); ctx.stroke();
      ctx.fillText(v.toFixed(Math.abs(max) < 3 ? 2 : 1), left - 4, Y(v) + 4);
    }
    if (min < 0 && max > 0) { ctx.strokeStyle = css('--muted'); ctx.beginPath(); ctx.moveTo(left, Y(0) + 0.5); ctx.lineTo(w - right, Y(0) + 0.5); ctx.stroke(); }

    const colors = [css('--cx'), css('--cy'), css('--cz')];
    ctx.lineWidth = 1.4;
    for (let s = 0; s < 3; s++) {
      const arr = buf.ch[c.first + s];
      ctx.strokeStyle = colors[s]; ctx.beginPath();
      for (let px = 0; px < pw; px++) {                     // min/max envelope per pixel column
        const a = i0 + Math.floor((px * n) / pw), b = Math.max(a + 1, i0 + Math.floor(((px + 1) * n) / pw));
        let vmin = Infinity, vmax = -Infinity;
        for (let k = a; k < b && k < buf.count; k++) { const v = arr[at(k)]; if (v < vmin) vmin = v; if (v > vmax) vmax = v; }
        if (vmin === Infinity) continue;
        ctx.moveTo(left + px + 0.5, Y(vmin)); ctx.lineTo(left + px + 0.5, Y(vmax) - 0.01);
      }
      ctx.stroke();
    }
  }

  function updateReadouts() {
    if (buf.count < 1) return;
    const k = at(buf.count - 1);
    const set = (id, v, d) => { $(id).textContent = v.toFixed(d); };
    ['gx', 'gy', 'gz'].forEach((id, i) => set('v-' + id, buf.ch[i][k], 3));
    ['ax', 'ay', 'az'].forEach((id, i) => set('v-' + id, buf.ch[3 + i][k], 2));
    ['px', 'py', 'pz'].forEach((id, i) => set('v-' + id, buf.ch[6 + i][k], 1));
    if (engine.calib) $('calib-text').textContent = `Calibrating… ${Math.round(engine.calib.progress * 100)}%`;
  }

  // ---- connection -------------------------------------------------------------------------------
  let lastData = 0, info = null;
  function connect() {
    const es = new EventSource('events');
    es.addEventListener('hello', (e) => { info = JSON.parse(e.data); showInfo(); });
    es.onmessage = (e) => {
      const m = JSON.parse(e.data);
      if (m.reset) { clearBuf(); engine.resetStream(); return; }
      lastData = performance.now();
      engine.feed(m.s);
    };
    es.onerror = () => { $('pill').textContent = 'server unreachable'; $('pill').className = 'pill bad'; };
  }
  function showInfo() {
    if (!info) return;
    const banner = $('banner');
    banner.hidden = info.kind !== 'sim';
    banner.textContent = 'SIMULATED DATA: axis directions here are invented for testing the page, not the real glasses.';
  }
  async function pollStatus() {
    try {
      info = await (await fetch('api/status', { cache: 'no-store' })).json();
      showInfo();
      const fresh = performance.now() - lastData < 2000;
      const pill = $('pill');
      if (info.connected && fresh) { pill.textContent = info.label; pill.className = 'pill good'; }
      else if (info.connected) { pill.textContent = 'connected, no data'; pill.className = 'pill bad'; }
      else { pill.textContent = 'waiting for source: ' + (info.error || info.label); pill.className = 'pill bad'; }
      $('rate').textContent = info.rate ? `${Math.round(info.rate)} samples/s` : '';
      const rec = $('btn-record');
      rec.textContent = info.recording ? 'Stop recording' : 'Record raw session';
    } catch (e) { $('pill').textContent = 'server unreachable'; $('pill').className = 'pill bad'; }
  }

  // ---- voice / beeps ----------------------------------------------------------------------------
  let audio = null;
  function beep(freq = 880, ms = 120) {
    try {
      audio = audio || new (window.AudioContext || window.webkitAudioContext)();
      const o = audio.createOscillator(), g = audio.createGain();
      o.frequency.value = freq; g.gain.value = 0.1; o.connect(g); g.connect(audio.destination);
      o.start(); o.stop(audio.currentTime + ms / 1000);
    } catch (e) { /* audio unavailable */ }
  }
  function say(text) {
    $('prompt').textContent = text; $('prompt').className = '';
    if ($('chk-voice').checked && 'speechSynthesis' in window) {
      speechSynthesis.cancel(); speechSynthesis.speak(new SpeechSynthesisUtterance(text));
    }
  }

  // ---- calibration ------------------------------------------------------------------------------
  $('btn-calib').onclick = () => {
    if (run) return;
    engine.startCalibration(3);
    $('calib-text').textContent = 'Calibrating… hold still';
  };
  $('btn-zero').onclick = () => engine.zeroAngles();
  engine.on('calibration', (r) => {
    const b = r.bias.map((v) => v.toFixed(4)).join(', ');
    const sd = Math.max(...r.std).toFixed(4);
    $('calib-text').textContent = r.ok
      ? `Calibrated. Gyro bias ${b} rad/s, noise ≤ ${sd}, |gravity| ${r.gravityMag.toFixed(2)} m/s².`
      : `Rejected: you moved (noise ${sd} rad/s, bias ${b}). Hold still and try again.`;
    $('btn-all').disabled = !r.ok;
    if (r.ok) { $('prompt').textContent = 'Ready. Press "Run all" or "Run" on a single test.'; render(); }
  });
  engine.on('ignored', () => { $('note').textContent = 'Ignored a very small movement; keep going.'; });

  // ---- tests ------------------------------------------------------------------------------------
  const DIRS = { yaw: ['left', 'right'], pitch: ['up', 'down'], roll: ['left shoulder', 'right shoulder'] };
  const DEFAULT_TARGET = { yaw: 45, pitch: 30, roll: 30 };
  let nextId = 1;
  const rows = [
    ['yaw', 'left'], ['yaw', 'right'], ['pitch', 'up'], ['pitch', 'down'], ['roll', 'left shoulder'], ['roll', 'right shoulder'],
  ].map(([motion, dir]) => ({ id: nextId++, motion, dir, target: DEFAULT_TARGET[motion], status: 'idle', go: null, ret: null, verdict: null }));

  let run = null;   // { queue, row }

  const instruction = (row, phase) => {
    if (phase === 'return') return 'Return to centre and hold still.';
    const t = `about ${row.target} degrees and hold still.`;
    if (row.motion === 'yaw') return `Turn your head ${row.dir} ${t}`;
    if (row.motion === 'pitch') return `Look ${row.dir} ${t}`;
    return `Tilt your head toward your ${row.dir} ${t}`;
  };

  function startRun(list) {
    if (run || !engine.calibrated || !list.length) return;
    run = { queue: list.slice(), row: null };
    $('btn-stop').disabled = false;
    nextRow();
  }
  function nextRow() {
    const row = run.queue.shift();
    if (!row) { stopRun('All tests done.'); beep(660, 250); return; }
    run.row = row; row.status = 'go'; row.go = row.ret = row.verdict = null;
    say(instruction(row, 'go'));
    engine.beginPhase('go');
    render();
  }
  function stopRun(message) {
    run = null; engine.cancelPhase();
    rows.forEach((r) => { if (r.status === 'go' || r.status === 'return') r.status = 'idle'; });
    $('btn-stop').disabled = true;
    $('prompt').textContent = message || 'Stopped.';
    if ('speechSynthesis' in window) speechSynthesis.cancel();
    render();
  }
  $('btn-stop').onclick = () => stopRun('Stopped.');
  $('btn-all').onclick = () => startRun(rows);
  $('btn-add').onclick = () => {
    rows.push({ id: nextId++, motion: 'yaw', dir: 'left', target: 45, status: 'idle', go: null, ret: null, verdict: null });
    render();
  };

  engine.on('phase', (r) => {
    if (!run) return;
    const row = run.row;
    $('note').textContent = '';
    if (row.status === 'go') {
      row.go = r; row.status = 'return'; beep(880);
      say(instruction(row, 'return'));
      engine.beginPhase('return');
    } else if (row.status === 'return') {
      row.ret = r; row.status = 'done'; beep(660);
      row.verdict = evaluate(row);
      setTimeout(() => { if (run) nextRow(); }, 1500);
    }
    render();
  });

  function evaluate(row) {
    const tol = Number($('tol').value) / 100, g = row.go, notes = [];
    const measured = Math.abs(g.deltaDominant), ratio = measured / row.target;
    let level = 'ok';
    const bump = (lv) => { if (lv === 'bad' || (lv === 'warn' && level === 'ok')) level = lv; };
    if (Math.abs(ratio - 1) > tol) { notes.push(`angle ${Math.round(ratio * 100)}% of target`); bump('warn'); }
    if (g.mixedRatio > 0.35) { notes.push('moved on more than one axis'); bump('warn'); }
    if (row.motion === 'yaw') {
      if (g.gravityRotDeg > 8) { notes.push(`gravity moved ${g.gravityRotDeg.toFixed(0)}° (did you also tilt?)`); bump('warn'); }
    } else if (Math.abs(g.gravityRotDeg - measured) > Math.max(5, 0.25 * measured)) {
      notes.push(`accelerometer saw ${g.gravityRotDeg.toFixed(0)}°, gyro ${measured.toFixed(0)}°`); bump('warn');
    }
    const closure = g.deltaDominant + row.ret.delta[g.dominantIndex];
    if (Math.abs(closure) > Math.max(3, 0.1 * measured)) { notes.push(`did not return to start (${fmt(closure)}°)`); bump('warn'); }
    return { level, notes, closure, ratio };
  }

  function render() {
    const tbody = $('tests');
    tbody.textContent = '';
    for (const row of rows) {
      const tr = document.createElement('tr');
      const td = (...kids) => { const c = document.createElement('td'); kids.forEach((k) => c.append(k)); tr.append(c); return c; };
      const busy = !!run;

      const motion = document.createElement('select');
      Object.keys(DIRS).forEach((m) => motion.add(new Option(m, m, false, m === row.motion)));
      motion.disabled = busy;
      const dir = document.createElement('select');
      const fillDirs = () => { dir.textContent = ''; DIRS[row.motion].forEach((d) => dir.add(new Option(d, d, false, d === row.dir))); };
      fillDirs(); dir.disabled = busy;
      motion.onchange = () => { row.motion = motion.value; row.dir = DIRS[row.motion][0]; row.target = DEFAULT_TARGET[row.motion]; render(); };
      dir.onchange = () => { row.dir = dir.value; };
      const target = document.createElement('input');
      target.type = 'number'; target.value = row.target; target.min = 5; target.max = 180; target.disabled = busy;
      target.onchange = () => { row.target = Number(target.value) || row.target; };

      td(motion); td(dir); td(target);
      td(document.createTextNode(row.status));
      const m = td();
      if (row.go) {
        m.append(document.createTextNode(`${row.go.dominant.toUpperCase()} ${fmt(row.go.deltaDominant)}°`));
        const d = document.createElement('div'); d.className = 'detail';
        d.textContent = `gravity ${row.go.gravityRotDeg.toFixed(0)}° · peak ${row.go.peakRate.toFixed(2)} rad/s` +
          (row.verdict ? ` · closure ${fmt(row.verdict.closure)}°` : '');
        m.append(d);
      }
      const v = td();
      if (row.verdict) {
        const b = document.createElement('span'); b.className = 'badge ' + row.verdict.level;
        b.textContent = row.verdict.level === 'ok' ? 'ok' : 'check';
        v.append(b);
        if (row.verdict.notes.length) { const d = document.createElement('div'); d.className = 'detail'; d.textContent = row.verdict.notes.join('; '); v.append(d); }
      }
      const actions = td();
      const runBtn = document.createElement('button'); runBtn.textContent = 'Run';
      runBtn.disabled = busy || !engine.calibrated; runBtn.onclick = () => startRun([row]);
      const del = document.createElement('button'); del.textContent = '✕'; del.title = 'Remove';
      del.disabled = busy; del.onclick = () => { rows.splice(rows.indexOf(row), 1); render(); };
      actions.append(runBtn, ' ', del);
      tbody.append(tr);
    }
    summarize();
  }

  // ---- summary / export -------------------------------------------------------------------------
  let exported = null;
  function summarize() {
    const done = rows.filter((r) => r.go && r.ret);
    const byMotion = {};
    done.forEach((r) => { (byMotion[r.motion] = byMotion[r.motion] || []).push(r); });
    const lines = [];
    const directions = {};
    for (const [motion, list] of Object.entries(byMotion)) {
      list.forEach((r) => {
        const key = `${motion}-${r.dir.replace(' shoulder', '')}`;
        directions[key] = { axis: r.go.dominant, sign: r.go.sign, measuredDeg: +r.go.deltaDominant.toFixed(1), target: r.target, check: r.verdict.level };
        lines.push(`${motion.padEnd(6)} ${r.dir.padEnd(15)} → gyro ${r.go.sign > 0 ? '+' : '−'}${r.go.dominant.toUpperCase()}   (${fmt(r.go.deltaDominant)}° measured, target ${r.target}°)`);
      });
      const axes = new Set(list.map((r) => r.go.dominant));
      const signs = list.map((r) => r.go.sign);
      if (axes.size > 1) lines.push(`  ! ${motion}: different axes were used, repeat the tests`);
      else if (list.length > 1 && new Set(signs).size === 1) lines.push(`  ! ${motion}: both directions gave the same sign, repeat the tests`);
      else lines.push(`  ✓ ${motion} is gyro ${[...axes][0].toUpperCase()}` + (list.length > 1 ? ', opposite directions have opposite signs' : ' (only one direction tested)'));
    }
    $('summary').textContent = lines.length ? lines.join('\n') : 'Run at least one test.';
    $('summary').className = lines.length ? '' : 'muted';
    exported = done.length ? {
      version: 1, created: new Date().toISOString(), source: info ? { kind: info.kind, label: info.label } : null,
      bias: engine.bias, directions,
      tests: done.map((r) => ({ motion: r.motion, direction: r.dir, targetDeg: r.target, go: r.go, ret: r.ret, verdict: r.verdict })),
    } : null;
    $('btn-save').disabled = $('btn-download').disabled = !exported;
  }
  $('btn-save').onclick = async () => {
    try {
      const res = await fetch('api/result', { method: 'POST', body: JSON.stringify(exported) });
      const j = await res.json();
      $('save-text').textContent = res.ok ? `Saved ${j.path}` : `Save failed: ${j.error}`;
    } catch (e) { $('save-text').textContent = 'Save failed: ' + e; }
  };
  $('btn-download').onclick = () => {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([JSON.stringify(exported, null, 2)], { type: 'application/json' }));
    a.download = 'axis-map.json'; a.click(); URL.revokeObjectURL(a.href);
  };
  $('btn-record').onclick = async () => {
    const res = await fetch('api/record', { method: 'POST', body: JSON.stringify({ on: !(info && info.recording) }) });
    const j = await res.json();
    $('save-text').textContent = j.recording ? `Recording to ${j.recording}` : 'Recording stopped.';
    pollStatus();
  };

  // ---- main loop --------------------------------------------------------------------------------
  let lastReadout = 0;
  function frame(now) {
    charts.forEach(drawChart);
    if (now - lastReadout > 100) { updateReadouts(); lastReadout = now; }
    requestAnimationFrame(frame);
  }
  render();
  connect();
  pollStatus();
  setInterval(pollStatus, 1000);
  requestAnimationFrame(frame);
})();
