
# Comet Striker adapter

## Source

`SRC-COMET-1.4` reports `QC_PASS_COMMIT_CANDIDATE` for the v1.4 EIC addition, while inherited preflight still blocks final deployment pending full runtime QC.

## Included

- review HTML: `comet/runtime_candidate/WB_COMET-STRIKER_CREATIVE-JOURNEY.html`;
- EIC schema, example, generator and print template;
- upstream tokens and selected QA evidence.

## Excluded

Raw conversation logs, opaque skill payloads, external deployment claims and upstream print PDFs.

## Promotion gate

Browser interaction, persistence, responsive layout, all print modes, accessibility, source-integrity and Director approval must pass. Until then: `CANDIDATE_FOR_REVIEW`.
