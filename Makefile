.PHONY: roadmap roadmap-build pr-fitcheck

# The dashboard is now a single hand-authored file: src/dashboard.html, embedded into the
# binary via include_str!. There is no stitch/build step anymore; edit src/dashboard.html
# directly and rebuild the binary (cargo install) to pick it up.

# Roadmap pipeline status page. Serves docs/roadmap.html and refreshes live from
# Linear when LINEAR_PAT is set in .env. Opens http://localhost:4318.
roadmap:
	@node tools/status-server.mjs

# Regenerate the committed docs/roadmap.html snapshot without starting the server.
roadmap-build:
	@node tools/status-data.mjs

# Run the local persona gate for the current branch's PR, or pass PR=<number>.
# A pass publishes the required "Local Fitcheck" status for the exact PR head.
pr-fitcheck:
	@bash scripts/pr-fitcheck.sh "$(PR)"
