# Forja

**An open-source backend in one file, written in Rust and WebAssembly.**
SQLite database, authentication, file storage, realtime and an admin dashboard,
in a single executable — with several projects per instance, an MCP server for
AI agents, and SDKs for nine languages.

Forja is a reimplementation inspired by [PocketBase](https://pocketbase.io)
(© Gani Georgiev, MIT). Its REST API follows PocketBase's, so existing clients
keep working.

> **Status: 0.0.1, early development.** Today: the workspace builds, `forja serve`
> runs and answers `/api/health`, and the admin shell compiles to WebAssembly.
> Forja reaches **1.0.0 only when it is stable** (criteria in
> [`docs/PLANO.md`](docs/PLANO.md) §6).

## What it will do

- **Everything PocketBase does:** collections (base, auth, view) with 14 field
  types, access rules, filters, expand, batch requests; email/password, OTP,
  MFA and OAuth2 with 30 providers; local or S3 file storage with thumbnails;
  realtime over SSE; backups, logs, cron, rate limits.
- **In Rust and WebAssembly:** an axum server with embedded SQLite, an admin
  dashboard in Leptos compiled to WASM, and server extensions as WASM modules
  instead of JavaScript.
- **Many projects in one instance**, each isolated, each at its own base URL
  (`https://host/p/<project>`).
- **A secure MCP server** for AI agents: read-only by default, scoped tokens,
  audit log, no raw SQL.
- **A full admin CLI** for projects, collections, records, tokens and backups.
- **SDKs** for JavaScript/TypeScript (React, Vue, Angular, Svelte, Node, Deno,
  Bun…), Kotlin (Android), Swift (iOS), Dart/Flutter, Rust, Java, Python, C#
  and Go — all built on one Rust core.

The complete list, with what is done and what is planned:
[`docs/FUNCIONALIDADES.md`](docs/FUNCIONALIDADES.md) (Portuguese).
How to use each part: [`docs/GUIA.md`](docs/GUIA.md).

## Quick start

```sh
git clone https://github.com/iramarfalcao/forjabase.git
cd forjabase
cargo run -p forja -- serve
curl http://127.0.0.1:8090/api/health
```

## Repository layout

```
crates/forja          the executable and CLI
crates/forja-core     data model (collections, fields, rules), no HTTP
crates/forja-server   HTTP API (axum), PocketBase-compatible routes
crates/forja-admin    admin dashboard, Leptos → WebAssembly
docs/                 features, plan, usage guide, status (Portuguese)
```

## License

MIT. See [`LICENSE`](LICENSE).
