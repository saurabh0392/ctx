.PHONY: dashboard dashboard-check e2e e2e-update

dashboard:
	@./scripts/stitch-dashboard.sh > src/dashboard.html
	@echo "Wrote src/dashboard.html"

dashboard-check:
	@./scripts/stitch-dashboard.sh | diff -u src/dashboard.html - || (echo "dashboard.html is stale; run make dashboard" >&2; exit 1)
	@echo "dashboard.html matches fragments"

e2e:
	@./scripts/run-e2e.sh

e2e-update:
	@./scripts/run-e2e.sh --update-snapshots
