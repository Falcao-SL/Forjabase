# Status — Forja

> Atualizado em **2026-10-21**. Este arquivo é a fonte única de "em que pé está".

| Campo | Valor |
|---|---|
| **Estágio** | **Em desenvolvimento** — 0.0.2: SQLite, superusuário, coleções base e registros com filtros, ordenação, paginação e regras, na API do PocketBase |
| **Plataformas** | Servidor: macOS, Linux, Windows (Rust); painel: navegador (WebAssembly) |
| **Stack** | Rust (axum, SQLite), Leptos → WebAssembly, wasmtime para extensões |
| **Site** | — (repositório: https://github.com/iramarfalcao/forjabase) |
| **Monetização** | Nenhuma: código aberto, MIT |
| **Próximo marco** | Fechar a 0.1: `PATCH` de coleção, `expand` e `fields`, operadores `?=`, banco de logs e o resto da CLI de superusuário |
| **Data-alvo** | A definir; 1.0.0 só quando estável (PLANO §6) |

## O que já está pronto

- Workspace com 4 crates, 23 testes passando, painel compilando para WASM.
- SQLite por projeto, trait `Banco` (pronto para Postgres por projeto).
- Superusuário (Argon2id, JWT), coleções base, registros com filtro do
  PocketBase convertido em SQL com parâmetros, ordenação, paginação, regras e
  campos ocultos.
- Cliente Python do PocketBase funciona contra o Forja (conferido em 2026-10-21).
- Documentação: [FUNCIONALIDADES](FUNCIONALIDADES.md), [PLANO](PLANO.md), [GUIA](GUIA.md).

## O que falta

Tudo marcado 📋 em [FUNCIONALIDADES.md](FUNCIONALIDADES.md), na ordem do PLANO §5.
