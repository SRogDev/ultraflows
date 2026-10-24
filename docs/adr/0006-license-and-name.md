# ADR-0006 — Name and license

- Status: **Decided** (2026-10-24, Roger — see Decision at the end)

- Name: **UltraFlows** is temporary (Roger's word). Rename in one sweep
  before the first public commit — late naming is the expensive option.
- License: crate manifests carry `MIT OR Apache-2.0` as a scaffold
  default only. Temporal (the reference system) is MIT; the Trigger.dev
  recon spec recommended Apache-2.0 for a portfolio runtime. Decide
  before any public release, not before building.

- **Decision (2026-10-24):** Roger chose the **Elastic License 2.0** —
  the same license as Shield AI — over the scaffold default and the
  Apache-2.0 recommendation: source-available protection (nobody may
  offer the software as a hosted service) for a runtime intended to
  underpin Polygrow. `LICENSE` carries the full text; the workspace
  manifest declares `Elastic-2.0`. The name **UltraFlows** ships as-is
  for the first publication; a later rename is a single sweep (GitHub
  redirects the old URL).
