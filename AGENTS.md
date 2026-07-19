# Project workflow

- Publish changes through a focused branch and pull request. Run the relevant local tests and
  `make pr-fitcheck PR=<number>` before merging.
- Let Copilot review each pull request once. Inspect and address or explicitly accept that one
  review, but do not request, wait for, or gate the merge on another Copilot pass after subsequent
  commits.
- After that single review, green required tests and the local fitcheck are sufficient to merge.
