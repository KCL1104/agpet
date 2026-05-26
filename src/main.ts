// Milestone 1, step 1: placeholder "pet" — a rounded rectangle that walks back
// and forth along the bottom of the screen with a small bob + footstep cycle.
// No ACP / Tauri commands yet; this is purely the rendering foundation.

const canvas = document.getElementById("pet-canvas") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const PET_W = 64; // body width (px)
const PET_H = 64; // body height (px)
const SPEED = 140; // walk speed (px/sec)
const BOB_HZ = 3; // bob/step cycles per second
const BOB_AMP = 6; // vertical bob amplitude (px)
const MARGIN_BOTTOM = 28; // gap from the bottom edge (px)

let x = 0; // body left, in CSS px
let dir: 1 | -1 = 1; // 1 = walking right, -1 = walking left
let baselineY = 0; // resting top of the body
let last = performance.now();

function resize() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(window.innerWidth * dpr);
  canvas.height = Math.floor(window.innerHeight * dpr);
  canvas.style.width = `${window.innerWidth}px`;
  canvas.style.height = `${window.innerHeight}px`;
  // Draw in CSS pixels regardless of display scaling.
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  baselineY = window.innerHeight - PET_H - MARGIN_BOTTOM;
}

function roundRect(c: CanvasRenderingContext2D, rx: number, ry: number, rw: number, rh: number, r: number) {
  c.beginPath();
  c.moveTo(rx + r, ry);
  c.arcTo(rx + rw, ry, rx + rw, ry + rh, r);
  c.arcTo(rx + rw, ry + rh, rx, ry + rh, r);
  c.arcTo(rx, ry + rh, rx, ry, r);
  c.arcTo(rx, ry, rx + rw, ry, r);
  c.closePath();
}

function draw(now: number) {
  const dt = Math.min((now - last) / 1000, 0.05); // clamp to survive tab stalls
  last = now;

  const w = window.innerWidth;

  // Advance and bounce at the screen edges, flipping facing direction.
  x += dir * SPEED * dt;
  if (x <= 0) {
    x = 0;
    dir = 1;
  } else if (x + PET_W >= w) {
    x = w - PET_W;
    dir = -1;
  }

  const phase = (now / 1000) * BOB_HZ * Math.PI * 2;
  const bob = Math.sin(phase) * BOB_AMP;
  const step = Math.sin(phase) * 6; // alternating footstep offset
  const y = baselineY + Math.abs(bob); // hop up, never sink below baseline

  ctx.clearRect(0, 0, w, window.innerHeight);

  // Feet (drawn first, behind the body).
  ctx.fillStyle = "#b8552a";
  ctx.fillRect(x + 12, y + PET_H, 14, 9 + step);
  ctx.fillRect(x + PET_W - 26, y + PET_H, 14, 9 - step);

  // Body.
  ctx.fillStyle = "#e8743b";
  roundRect(ctx, x, y, PET_W, PET_H, 14);
  ctx.fill();

  // Eye on the leading edge to show which way it faces.
  const eyeCx = dir === 1 ? x + PET_W - 18 : x + 18;
  const eyeCy = y + 24;
  ctx.fillStyle = "#ffffff";
  ctx.beginPath();
  ctx.arc(eyeCx, eyeCy, 8, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "#1a1a1a";
  ctx.beginPath();
  ctx.arc(eyeCx + dir * 2.5, eyeCy, 4, 0, Math.PI * 2);
  ctx.fill();

  requestAnimationFrame(draw);
}

window.addEventListener("resize", resize);
resize();
requestAnimationFrame(draw);
