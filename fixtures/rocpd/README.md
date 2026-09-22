# ROCPD schema fixture

`sdk-3.0.4.db` contains synthetic records generated from the local ROCm SDK's unchanged `rocpd_tables.sql` (schema 3.0.4) and the `insert_minimal_data` function in `tests/rocpd-back-compat/make_db.py`. No real process environment or profiling data is included. The table template's UUID/GUID placeholders were replaced with the fixture generator's deterministic identifiers.

Source: https://github.com/ROCm/rocm-systems/tree/develop/projects/rocprofiler-sdk

The fixture includes two kernel dispatches and two timestamped SPM values in different shader-engine instances. Upstream's MIT notice is retained in the repository's third-party notices.

Local source checkout HEAD at generation: `7dda3ac6cfe6bbe0b7f08c23a67cfa118d8641a1`. Exact source SHA-256 values (which also identify any local changes):

- SQL schema: `d903e36c4ce9c8cf5c5fdd45ac126bff366354daeb2ac90ac492c20fd908138e`
- Fixture generator: `d917f725b9ccb6d155a6545d373273dff4226a3999a202cae674ac279cf6f3c7`
- Database: `0177e2d11245ad2e5e9d9b9e451d10b7ecc00adceb6314ac9b0ca629302612cc`

The browser projection is regenerated with `cargo run -q -p profile-core -- fixtures/rocpd/sdk-3.0.4.db > fixtures/rocpd/sdk-3.0.4.json`. It tests frontend rendering; Rust integration tests independently read the SQLite database.
