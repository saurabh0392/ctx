// Active-surface claim lint. Historical ADRs and research notes may preserve superseded language;
// the shipped dashboard, README, portfolio, and report exporter may not silently reintroduce it.

import { readFileSync } from 'fs';

const files = ['README.md', 'src/dashboard.html', 'src/report.rs', 'docs/portfolio/index.html'];
const forbidden = [
  { re: /\bprove(?:d|s|n)? safe\b/i, why: 'use observed-harm-bar language, not causal safety proof' },
  { re: /\bproven across\b|\bfinish the proof\b|\bproof (?:is )?(?:not )?locked\b/i, why: 'describe the evidence threshold without causal-certainty language' },
  { re: /\bcould safely reclaim\b|\bsessions show (?:that )?(?:it|trimming) was safe\b/i, why: 'separate transform eligibility from observed-harm activation' },
  { re: /\bproof, tool by tool\b/i, why: 'call this evidence, not proof' },
  { re: /\bnothing leaves this machine\b/i, why: 'exports and explicit preview/send flows exist' },
  { re: /\bno telemetry\b/i, why: 'say no background telemetry; opt-in check-ins exist' },
  { re: /linkedin\.com\/in\/your-handle|github\.com\/your-handle/i, why: 'placeholder social link' },
];

let failures = 0;
for (const file of files) {
  const lines = readFileSync(file, 'utf8').split(/\r?\n/);
  lines.forEach((line, index) => {
    for (const rule of forbidden) {
      if (!rule.re.test(line)) continue;
      console.error(`${file}:${index + 1}: ${rule.why}\n  ${line.trim().slice(0, 220)}`);
      failures++;
    }
  });
}

if (failures) {
  console.error(`\nclaim consistency failed: ${failures} issue(s)`);
  process.exit(1);
}
console.log(`claim consistency passed (${files.length} active surfaces)`);
