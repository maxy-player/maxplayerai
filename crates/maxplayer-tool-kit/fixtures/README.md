# Fixtures

`seller-tool-config.json` is the seller's offering: the operations this seller sells, the vendor
base URL, and the parameter grammar for each operation. It is **seller configuration** — there is
no per-job counterpart to any of it, and nothing in it is issued, metered or expired per job.

`vendor_base_url` here is a placeholder. Both the test suite and the Docker demo override it with
`--vendor-base-url`, because the fake vendor binds an ephemeral port.

## No credential file is committed here, deliberately

The vendor service and the holder both need a synthetic credential file. It is **generated at run
time** — by `tests/common/mod.rs` into a temp directory, and by `scripts/demo.sh` into the run's
own state directory. Nothing credential-shaped is committed, so no scanner has to decide whether
this one was real and no reader has to take our word for it.

The credential is synthetic in the only sense that matters: the account it opens exists solely
inside `vendor-service`, a fake in this crate, and no live account, network egress or spend is
involved anywhere in this kit.
