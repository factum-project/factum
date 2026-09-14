const puppeteer = require('puppeteer');
const fs = require('fs');
const path = require('path');

// Revised animation lines per review feedback:
// 1. Removed unverified "-62% tokens" claim (issue #9 — not measured yet)
// 2. Changed "model: gpt-4" to "model: claude-sonnet-4" (actually used host)
// 3. Insert flow: natural language prompt -> tool call (closer to real MCP UX)
// 4. "simulated session" badge added in HTML titlebar
const lines = [
  // === Section 1: Insert ===
  { t: 600,  cls: "prompt",    text: "> Add ACME's major shareholder to the KB" },
  { t: 400,  cls: "label",     text: "  \u2699 factum_insert(...)" },
  { t: 500,  cls: "fade",      text: "  (shareholder-major @ACME @FOUNDER-1 0.73)" },
  { t: 400,  cls: "fade",      text: "  conf: 0.85  src: doc002  model: claude-sonnet-4" },
  { t: 600,  cls: "mcp",       text: "  \u2713 Node n001 inserted" },

  // === Section 2: Query ===
  { t: 700,  cls: "prompt",    text: "> Who is ACME's major shareholder?" },
  { t: 400,  cls: "label",     text: "  \u2699 factum_query(...)" },
  { t: 700,  cls: "mcp",       text: "  \u2190 1 result (canonical 7-tuple form)" },
  { t: 500,  cls: "mcp",       text: "  @FOUNDER-1 holds 0.73 since 2001-03-15" },

  // === Section 3: Provenance ===
  { t: 300,  cls: "section",   text: "  \u2500\u2500 provenance \u2500\u2500" },
  { t: 500,  cls: "highlight", text: "  source: doc002 (extracted [100:200])" },
  { t: 500,  cls: "highlight", text: "  model: claude-sonnet-4 (2025-10)" },
  { t: 500,  cls: "highlight", text: "  confidence: 0.85  authority: 0.8" },

  // === Section 4: Retract ===
  { t: 700,  cls: "prompt",    text: "> Retract n001 \u2014 doc superseded" },
  { t: 400,  cls: "label",     text: "  \u2699 factum_retract(...)" },
  { t: 600,  cls: "mcp",       text: "  \u2713 Retracted n001" },

  // === Section 5: Query again ===
  { t: 700,  cls: "prompt",    text: "> Query again" },
  { t: 400,  cls: "label",     text: "  \u2699 factum_query(...)" },
  { t: 700,  cls: "mcp",       text: "  \u2190 0 results (audit trail kept)" },

  // === Footer ===
  { t: 500,  cls: "dim",       text: "  factum-mcp-server \u00b7 112 tests \u00b7 MIT" },
  { t: 300,  cls: "dim",       text: "  github.com/factum-project/factum" },
];

async function captureFrames() {
  const htmlPath = path.resolve(__dirname, 'terminal-animation.html');
  const framesDir = path.resolve(__dirname, 'frames');

  if (fs.existsSync(framesDir)) {
    fs.rmSync(framesDir, { recursive: true });
  }
  fs.mkdirSync(framesDir, { recursive: true });

  const browser = await puppeteer.launch({
    headless: 'new',
    args: ['--no-sandbox', '--disable-setuid-sandbox']
  });

  const page = await browser.newPage();
  await page.setViewport({ width: 800, height: 450, deviceScaleFactor: 2 });
  await page.goto('file://' + htmlPath, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('#body', { timeout: 5000 });

  await page.evaluate(() => {
    let id = window.setTimeout(() => {}, 0);
    while (id--) { window.clearTimeout(id); }
    document.getElementById('body').innerHTML = '';
  });

  console.log(`Animation lines: ${lines.length}`);

  let frameIndex = 0;

  async function shot() {
    const framePath = path.join(framesDir, `frame_${String(frameIndex).padStart(4, '0')}.png`);
    await page.screenshot({
      path: framePath,
      clip: { x: 0, y: 0, width: 800, height: 450 }
    });
    frameIndex++;
  }

  async function addLine(line) {
    await page.evaluate((lineData) => {
      const body = document.getElementById('body');
      const el = document.createElement('div');
      el.className = 'line ' + lineData.cls;
      el.textContent = lineData.text;
      body.appendChild(el);
      while (body.children.length > 14) {
        body.removeChild(body.firstChild);
      }
    }, line);
  }

  // First frame must have content — it becomes the static cover frame
  // in README loading placeholders, social media previews, GitHub thumbnails.
  // Pre-inject the first prompt line so frame 0 tells a story immediately.
  await addLine(lines[0]);

  // Short pause on the first line (reading time)
  for (let i = 0; i < 6; i++) { await shot(); }

  // Skip lines[0] in the main loop (already added)
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i];
    const framesForThisLine = Math.max(2, Math.round(line.t / 100));
    await addLine(line);
    for (let f = 0; f < framesForThisLine; f++) { await shot(); }
    if (i % 5 === 0 || i === lines.length - 1) {
      console.log(`  Line ${i + 1}/${lines.length} (total ${frameIndex})`);
    }
  }

  // End pause
  for (let i = 0; i < 8; i++) { await shot(); }

  console.log(`Total frames: ${frameIndex}`);

  const sharp = require('sharp');
  const files = fs.readdirSync(framesDir).filter(f => f.endsWith('.png')).sort();
  const meta = await sharp(path.join(framesDir, files[0])).metadata();
  console.log(`Frame size: ${meta.width}x${meta.height}`);

  const f0 = await sharp(path.join(framesDir, files[0])).stats();
  const fMid = await sharp(path.join(framesDir, files[Math.floor(files.length/2)])).stats();
  const fLast = await sharp(path.join(framesDir, files[files.length-1])).stats();
  console.log('Frame 0:', f0.channels.map(c => c.mean.toFixed(1)).join(', '));
  console.log('Frame mid:', fMid.channels.map(c => c.mean.toFixed(1)).join(', '));
  console.log('Frame last:', fLast.channels.map(c => c.mean.toFixed(1)).join(', '));

  await browser.close();
  console.log('Done.');
}

captureFrames().catch(console.error);
