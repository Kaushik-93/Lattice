# evals (phase 4)

Question → expected-result pairs on demo datasets (TPC-H and a retail demo shaped like the design's
"Acme Retail"), plus a runner that asks each question through the gateway, compares the resulting
VisualSpec and data with the expectation, and records accuracy per commit. CI runs it; the phase 4
exit criterion is ≥ 85% accuracy with no visual ever using unapproved logic without a badge.
