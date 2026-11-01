# Contributing to TAPEWORM

Thanks for helping. Bug reports, fixes, documentation and well-argued sound improvements are welcome.

## Workflow

1. Open an issue first for anything larger than a small fix, so the approach can be agreed.
2. Fork, create a branch, make focused commits.
3. Open a pull request describing **what** changed, **why**, and **how you tested it**.
4. Keep pull requests small: one concern per pull request.

## Development setup

See [BUILDING.md](BUILDING.md). In short:

```sh
cargo test --locked --lib
cargo xtask bundle tapeworm --release --locked
```

## Testing expectations

* `cargo test --locked --lib` must pass.
* New DSP behaviour needs a unit test next to the code (see the existing tests in `src/dsp/` and
  `src/lib.rs`).
* The audio thread must not allocate: debug builds abort on allocations in `process()` (nih-plug
  `assert_process_allocs`). Run a debug build in a host at least once when touching `process()`.

## Changes that affect the sound

Existing projects depend on TAPEWORM sounding the same. For any change to `src/dsp/`, to `process()`,
or to parameter definitions:

* **Compare before and after** with the black-box harness in `tests/regression/`
  (`compare_builds.py reference.clap candidate.clap OUTDIR`). A refactor must be bit-identical
  (`bit_identical` equal to the number of cases). An intended sound change must say which settings change,
  by how much, and why.
* **Never change parameter IDs, ranges, defaults or order**, the CLAP ID (`com.messyquest.tapeworm`) or the
  VST3 class ID. They are how hosts store automation and saved projects.
* Check 44.1, 48, 96 and 192 kHz: anything defined in time (ms, Hz, µs) must stay the same in time at every
  rate.
* Parameter changes must stay click-free under automation.
* Do not change the editor artwork, layout or fonts without discussing it first.

## Style

Follow the existing code: `rustfmt` defaults, small DSP structs with `reset()` / `process()` methods,
comments that explain *why*. No new dependencies without discussion.

## Licensing of contributions

TAPEWORM is licensed GPL-3.0-or-later. By submitting a contribution you agree that it is licensed under
the same terms ("inbound = outbound") and that you have the right to submit it. Please add a
`Signed-off-by:` line to your commits (`git commit -s`) to certify the
[Developer Certificate of Origin](https://developercertificate.org/). There is no contributor licence
agreement.

Do not contribute code, artwork or fonts that you did not create unless their licence is compatible with
GPL-3.0-or-later and you include the required notices.
