// Build the ctx demo video. Each scene's on-screen duration is derived from its own narration audio,
// so picture and voice stay locked in sync (no global timeline to drift). Narration via ElevenLabs
// (natural voice; key from .env), falling back to macOS `say`. Assembly via ffmpeg. Output: ctx-demo.mp4.
import { execFileSync } from 'child_process';
import { writeFileSync, readFileSync, existsSync, mkdirSync } from 'fs';
import { createHash } from 'crypto';

const DIR = process.argv[2];
const VARIANT = process.argv[3] || 'alpha'; // 'alpha' (install -> savings -> report) or 'portfolio'
if (!DIR) throw new Error('usage: node build-demo.mjs <workdir> [alpha|portfolio]');

// --- ElevenLabs config -----------------------------------------------------
const VOICE_ID = 'cjVigY5qzO86Huf0OWal'; // Eric - smooth, trustworthy, american. Swap for another male voice here.
const MODEL_ID = 'eleven_multilingual_v2';
function loadKey() {
  const p = '/Users/chikkupikku/Projects/ctx/.env';
  if (!existsSync(p)) return null;
  const m = readFileSync(p, 'utf8').match(/^\s*(?:ELEVENLABS_PAT|ELEVENLABS_API_KEY)\s*=\s*(.+)\s*$/m);
  return m ? m[1].trim().replace(/^["']|["']$/g, '') : null;
}
const KEY = loadKey();

// Speak the exact figures the dashboard is showing (written by capture.mjs), so voice matches picture.
const NUM = JSON.parse(readFileSync(`${DIR}/numbers.json`, 'utf8'));
const ONES = ['zero','one','two','three','four','five','six','seven','eight','nine','ten','eleven',
  'twelve','thirteen','fourteen','fifteen','sixteen','seventeen','eighteen','nineteen'];
const TENS = ['','','twenty','thirty','forty','fifty','sixty','seventy','eighty','ninety'];
function words(n) {
  n = Math.round(n);
  if (n < 20) return ONES[n];
  if (n < 100) { const t = Math.floor(n / 10), o = n % 10; return TENS[t] + (o ? ' ' + ONES[o] : ''); }
  const h = Math.floor(n / 100), r = n % 100; return ONES[h] + ' hundred' + (r ? ' ' + words(r) : '');
}
function spoken(disp) {
  const m = String(disp).match(/^([\d.]+)([MK]?)$/i);
  if (!m) return disp;
  const [, numStr, suf] = m;
  const [ip, dp] = numStr.split('.');
  let s = words(parseInt(ip, 10));
  if (dp && parseInt(dp, 10) !== 0) s += ' point ' + dp.split('').map((d) => ONES[+d]).join(' ');
  const scale = suf.toUpperCase() === 'M' ? ' million' : suf.toUpperCase() === 'K' ? ' thousand' : '';
  return s + scale;
}
const OUTPUT = spoken(NUM.see_output), INPUT = spoken(NUM.see_input), RECLAIMED = spoken(NUM.home_reclaimed);

const FPS = 30, LEAD = 0.35, TAIL = 0.75, FADE = 0.4;

// Scene = still image + the line spoken over it. The brand is spelled "C T X" so the voice reads it
// letter by letter. Dashboard figures are templated from numbers.json so voice matches the screens.
const scenesAlpha = [
  { img: 'slide-title.png', vo:
    'This is C T X. It keeps your coding agent’s context lean, and it shows you the bill. Here is the whole picture, on real data.' },
  { img: 'install-panel.png', vo:
    'Getting started is three commands. Clone the repo, run the installer, and C T X setup. No Rust, no config. It wires into your agent and starts watching in the background.' },
  { img: 'diagram-arch.png', vo:
    'C T X is a local hook. It sits between your agent and its tools, and it works on every result before your context ever grows. No account. No telemetry. Nothing leaves your machine.' },
  { img: 'shot-see.png', vo:
    `Every turn, your agent pays two taxes. Output: everything it reads back from a tool, ${OUTPUT} tokens here. And input: the tool menu it carries on every request, whether it calls those tools or not, ${INPUT} tokens each time.` },
  { img: 'diagram-trim.png', vo:
    'Start with output. When a tool returns a wall of text, C T X shortens it right where the agent reads it, and leaves a marker. The agent keeps working on the lean version.' },
  { img: 'panel-trim.png', vo:
    'Here it is on a real git status. Two thousand three hundred characters, down to five hundred and eighty eight. The list is summarized, and the marker says exactly how to get the rest.' },
  { img: 'panel-expand.png', vo:
    'If the agent needs the whole thing, it calls C T X expand with that id. Every byte comes back, all two thousand three hundred characters. The trim was never a delete. It was a fold.' },
  { img: 'shot-see2.png', vo:
    'And C T X only trims what your own runs prove is safe. Your file reads are almost entirely trimmable. Your edits, barely at all. It measures the difference. It does not guess.' },
  { img: 'diagram-earn.png', vo:
    'That safety is a ladder. A tool is watched, then trialed on a slice of your work, then it has to prove you never re-read to recover. Only then does it trim for good. A tool in use is never cut.' },
  { img: 'shot-save.png', vo:
    'The dashboard shows every tool on that ladder, in real time. What is earning its cut, what is still on trial, and what C T X will never touch.' },
  { img: 'diagram-restore.png', vo:
    'The tool menu works the same way. Idle tools get pruned to reclaim their tokens. But if the agent reaches for one, C T X brings it back.' },
  { img: 'panel-tools.png', vo:
    'Here, Linear was pruned. But C T X denied only its five idle tools. The ones you actually use stayed connected. A used server is never disconnected.' },
  { img: 'panel-restore.png', vo:
    'And when the agent does need a pruned tool, C T X restore un-prunes it for the next session, and carries a note of what you were doing, so the work picks up right where it left off.' },
  { img: 'shot-home.png', vo:
    `Put it all together. On this machine, C T X has reclaimed ${RECLAIMED} tokens so far, and you never changed how you work.` },
  { img: 'diagram-support.png', vo:
    'One honest note on where it runs. In Claude Code, CLI or IDE, you get all of it: the trimming, the pruning, and the dashboard. Claude Desktop has no hooks, so there you get the C T X tools and the dashboard, the insight and the recovery, but not automatic trimming. macOS and Linux today. Windows is on the way.' },
  { img: 'slide-trust.png', vo:
    'All of it is reversible, and all of it is local. When C T X is unsure, it leaves your context alone. It fails closed, toward keeping your data, never toward losing it.' },
  { img: 'shot-report.png', vo:
    'For the alpha, that is where you come in. When something feels off, a trim that hid too much, a tool you missed, a number you do not trust, press Report. It files an issue for us, screenshot and all. No repo access, no account.' },
  { img: 'slide-outro.png', vo:
    'Install C T X, use your agent exactly as you do today, and let it run for a week. Keep the context lean. Stay in control. See the bill.' },
];

// Portfolio cut: shorter, no install, no report, ends on the light/dark showcase.
const scenesPortfolio = [
  { img: 'slide-title.png', vo:
    'This is C T X. It keeps your coding agent’s context lean, and shows you the bill.' },
  { img: 'diagram-arch.png', vo:
    'It runs as a local hook, between your agent and its tools. Everything stays on your machine.' },
  { img: 'shot-see.png', vo:
    `Every turn, your agent resends more than it needs. The output it reads back from tools, ${OUTPUT} tokens here, and the tool menu it carries whether it uses it or not.` },
  { img: 'diagram-trim.png', vo:
    'C T X trims the output right where the agent reads it, and leaves a marker. The context stays small.' },
  { img: 'panel-expand.png', vo:
    'And it is reversible. One call brings the full text back, every byte. Nothing is ever lost.' },
  { img: 'diagram-earn.png', vo:
    'It only trims what your own sessions prove is safe. A tool in use is never cut.' },
  { img: 'shot-home.png', vo:
    `The result, on this machine: ${RECLAIMED} tokens reclaimed, with no change to how you work.` },
  { img: 'diagram-support.png', vo:
    'Where it runs, honestly. Full in Claude Code, CLI or IDE. On Claude Desktop, the tools and the dashboard, not the automatic trimming. macOS and Linux today, Windows coming.' },
  { img: 'theme-split.png', vo:
    'And it is a dashboard you will actually open. Light, or dark, whichever you live in.' },
  { img: 'slide-portfolio-outro.png', vo:
    'C T X. See where your context goes, and take it back.' },
];

const scenes = VARIANT === 'portfolio' ? scenesPortfolio : scenesAlpha;

function sh(cmd, args) {
  return execFileSync(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'] }).toString();
}
function audioDur(f) {
  return parseFloat(sh('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'default=nw=1:nk=1', f]).trim());
}

async function elevenTTS(text, outMp3) {
  const res = await fetch(`https://api.elevenlabs.io/v1/text-to-speech/${VOICE_ID}?output_format=mp3_44100_128`, {
    method: 'POST',
    headers: { 'xi-api-key': KEY, 'content-type': 'application/json' },
    body: JSON.stringify({
      text,
      model_id: MODEL_ID,
      voice_settings: { stability: 0.45, similarity_boost: 0.8, style: 0.0, use_speaker_boost: true },
    }),
  });
  if (!res.ok) throw new Error(`ElevenLabs ${res.status}: ${(await res.text()).slice(0, 200)}`);
  writeFileSync(outMp3, Buffer.from(await res.arrayBuffer()));
}

const segments = [];
for (let i = 0; i < scenes.length; i++) {
  const s = scenes[i];
  const n = String(i + 1).padStart(2, '0');
  const wav = `${DIR}/narr_${VARIANT}_${n}.wav`;
  const seg = `${DIR}/seg_${VARIANT}_${n}.mp4`;

  // Narration -> wav. ElevenLabs when a key is present, else macOS `say`. Narration is cached by a hash
  // of (voice, model, text): a rebuild only calls the API for lines that actually changed, so adding a
  // scene is one call, not a full-deck regeneration. This keeps us well clear of automated abuse flags.
  if (KEY) {
    const cacheDir = `${DIR}/tts-cache`;
    mkdirSync(cacheDir, { recursive: true });
    const hash = createHash('sha1').update(`${VOICE_ID}:${MODEL_ID}:${s.vo}`).digest('hex');
    const cached = `${cacheDir}/${hash}.mp3`;
    if (existsSync(cached)) {
      console.log(`scene ${n}: narration cached`);
    } else {
      await elevenTTS(s.vo, cached);
      // Throttle real API calls so a rebuild never looks like an automated burst.
      await new Promise((r) => setTimeout(r, 1500));
    }
    sh('ffmpeg', ['-y', '-i', cached, '-ar', '48000', '-ac', '2', wav]);
  } else {
    const aiff = `${DIR}/narr_${VARIANT}_${n}.aiff`;
    sh('say', ['-v', 'Samantha', '-r', '172', '-o', aiff, s.vo]);
    sh('ffmpeg', ['-y', '-i', aiff, '-ar', '48000', '-ac', '2', wav]);
  }

  const dur = audioDur(wav) + LEAD + TAIL;
  const frames = Math.round(dur * FPS);
  const vf =
    `zoompan=z='min(zoom+0.00035,1.06)':d=${frames}:x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':s=1920x1080:fps=${FPS},` +
    `fade=t=in:st=0:d=${FADE},fade=t=out:st=${(dur - FADE).toFixed(2)}:d=${FADE}`;
  const af = `adelay=${Math.round(LEAD * 1000)}|${Math.round(LEAD * 1000)},apad`;

  sh('ffmpeg', [
    '-y',
    '-loop', '1', '-t', dur.toFixed(2), '-i', `${DIR}/${s.img}`,
    '-i', wav,
    '-filter_complex', `[0:v]${vf}[v];[1:a]${af}[a]`,
    '-map', '[v]', '-map', '[a]',
    '-t', dur.toFixed(2),
    '-c:v', 'libx264', '-preset', 'medium', '-crf', '18', '-pix_fmt', 'yuv420p', '-r', String(FPS),
    '-c:a', 'aac', '-b:a', '192k',
    seg,
  ]);
  segments.push(seg);
  console.log(`scene ${n}: ${(dur - LEAD - TAIL).toFixed(1)}s voice -> ${dur.toFixed(1)}s scene`);
}

const listFile = `${DIR}/concat_${VARIANT}.txt`;
writeFileSync(listFile, segments.map((s) => `file '${s}'`).join('\n') + '\n');
const finalMp4 = `${DIR}/ctx-demo-${VARIANT}.mp4`;
sh('ffmpeg', ['-y', '-f', 'concat', '-safe', '0', '-i', listFile, '-c', 'copy', finalMp4]);
console.log('WROTE', finalMp4, '|', audioDur(finalMp4).toFixed(1), 's |', KEY ? 'ElevenLabs' : 'say');
