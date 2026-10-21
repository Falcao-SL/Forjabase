# Changelog

## 0.0.3 — 2026-10-21

- Três motores de banco com a mesma API, escolhidos com `--engine` (ou `FORJA_ENGINE`):
  - **SQLite** (rusqlite), o padrão;
  - **Turso**, que abre o mesmo arquivo do SQLite;
  - **PostgreSQL / Supabase** (`FORJA_PG_URL`), com TLS por rustls e um esquema `forja_<projeto>` por projeto.
- `store::Banco` passa a ser um só, assíncrono, sobre o trait `motor::Executor`; a lógica de coleções, registros, regras e validação não depende mais do motor.
- Bateria de conformidade (`store::conformidade`) que todo motor precisa passar; o CI ganhou um job com Postgres 17.
- Ordenação estável por `"id"` em vez de `rowid`, igual em todos os motores.

## 0.0.2 — 2026-10-21

- SQLite por projeto (`forja_data/default/data.db`, WAL) atrás do trait `Banco`, pronto para um Postgres opcional por projeto.
- Superusuários: `forja superuser upsert`, login (`auth-with-password`) e `auth-refresh`; senha em Argon2id; token JWT invalidado ao trocar a senha.
- Coleções base: listar, criar, ver e apagar; campos `text`, `editor`, `number`, `bool`, `email`, `url`, `date`, `select` e `json` com validação e códigos de erro do PocketBase.
- Registros: listar (paginação, `filter`, `sort`, `skipTotal`), criar, ver, atualizar e apagar, com regras de acesso por operação e campos `hidden`.
- Filtros do PocketBase convertidos em SQL com parâmetros (SQLite e Postgres), recusando campos desconhecidos.
- `--dir` para a pasta de dados.

## 0.0.1 — 2026-10-21

- Workspace Rust com quatro crates: `forja`, `forja-core`, `forja-server`, `forja-admin`.
- `forja serve` (porta padrão 8090) e `GET /api/health` no formato do PocketBase.
- Tipos de coleção (base, auth, view) e os 14 tipos de campo, com os nomes do PocketBase.
- Casca do painel administrativo em Leptos, compilando para `wasm32-unknown-unknown`.
- Documentação: funcionalidades, plano, guia de uso e status.
