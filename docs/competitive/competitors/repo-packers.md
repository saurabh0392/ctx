# Repo and context packers: Repomix, code2prompt, files-to-prompt, Aider repo map

- Category: repo / context packer (retrieval side)
- Classification: adjacent (same job-to-be-done of "fit the codebase in context," different point in the path)
- One-liner: tools that turn a repository into a compact, AI-ready prompt, by packing files, building a structural map, or compressing with tree-sitter.
- URL / repo / docs: https://github.com/yamadashy/repomix, https://github.com/mufeedvh/code2prompt, https://github.com/simonw/files-to-prompt, Aider repo map (https://github.com/Aider-AI/aider). Adjacent: Gitingest, RepoPrompt, Continue.dev, Cline, Roo Code.
- Maturity signals (observed via GitHub API, 2026-06-13 unless noted):
  - Repomix: 26,245 stars, MIT, very active (last push 2026-06-13), v1.14.1 (2026-05-27). Tree-sitter "compress" mode claims about 70% token reduction (third-party, Ry Walker research, 2026). ~255k npm downloads/month (third-party).
  - code2prompt: 7,405 stars, MIT, Rust, latest tagged release v4.2.0 (2025-12-11); slowing (third-party).
  - files-to-prompt: about 2.6k stars (third-party), simple file concatenation, by Simon Willison.
  - Aider repo map: built into Aider (about 43k stars for Aider overall, third-party); tree-sitter tag map plus PageRank, dynamically selected per chat.
- License / pricing: all MIT or similar, free.

## What it does and how (mechanism)

These shape what enters context before or around the agent loop, on the retrieval side:

- Repomix packs an entire repo into one XML or Markdown file optimized for the model, with secret scanning and an optional tree-sitter compress mode that keeps structure and drops bodies. Runs as CLI, MCP server, or Chrome extension.
- code2prompt serializes a codebase into a templated prompt with token counting and Handlebars templates (Rust, fast).
- files-to-prompt concatenates selected files into one prompt. Minimal.
- Aider repo map is not a separate tool: it extracts a tag map of all definitions and references with tree-sitter, ranks with PageRank, and dynamically picks the most relevant structural context for each chat. About 1k tokens for a whole-repo map vs 50k-500k for a full dump.

Where they sit: upstream of the conversation. They decide what code the agent sees in the first place. ctx sits downstream, trimming what tools return during the loop.

## Claimed results vs verifiable results

- Claimed (vendor / third-party): Repomix tree-sitter compress about 70% reduction; Aider repo map about 1k tokens for a whole-repo structural view. Source: project docs and Ry Walker's comparison, accessed 2026-06-13. Label: vendor and third-party claim; the structural-map token figures are mechanically plausible.
- Verifiable: stars, licenses, release cadence observed via GitHub API.

## Strengths

- Solve the "get the right code into context" problem well, especially Aider's dynamic repo map (the most sophisticated).
- Mature, popular, free, easy.
- Repomix's MCP server and skills mean agents can call it dynamically.

## Weaknesses and blind spots (relative to ctx's problem)

- They are mostly one-shot or up-front. A full repo dump (Repomix, code2prompt, files-to-prompt) can be huge (50k-500k tokens) and does nothing about per-turn tool-output bloat during the agent loop.
- No proof, no per-tool governance, no protection of in-flight reads.
- They shape the input; they do not manage the growing conversation or the tool results that accumulate turn by turn.

## Overlap with ctx

Low and complementary. ctx does not pack repos or build maps; it trims tool outputs and filters MCP schemas during the loop. A developer can use Repomix or Aider's map to seed context and run ctx to keep the loop lean. The shared job-to-be-done ("don't drown the model in tokens") is why they belong in the same landscape, but they do not collide.

## Where ctx is better / where it is worse

Better (for the loop): ctx manages tokens as the session runs, with proof and protection. Packers are blind to what happens after the first prompt.
Worse (for input shaping): ctx does nothing to decide which files enter context. Aider's repo map and Repomix do that well and ctx should not rebuild it.

## Threat level to ctx: low

Different point in the path. No realistic path to one absorbing the other, though an agent could bundle both behaviors.

## What ctx should learn or steal

- Aider's dynamic, structure-aware selection is the gold standard for relevance. ctx's tool-output trimming could borrow the instinct: keep the structurally important lines (signatures, errors, changed regions), drop the rest.
- Repomix's tree-sitter compress mode is a concrete, popular implementation of structure-aware code reduction. Worth studying for ctx's `Read` trimming.
- Position as complementary in 30-market-and-segments.md. These tools share ctx's buyer; they are not rivals.
