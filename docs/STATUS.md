# Status — Forja

> Atualizado em **2026-10-21**. Este arquivo é a fonte única de "em que pé está".

| Campo | Valor |
|---|---|
| **Estágio** | **Em desenvolvimento** — 0.0.1, esqueleto: `serve`, `/api/health`, modelo e casca do painel |
| **Plataformas** | Servidor: macOS, Linux, Windows (Rust); painel: navegador (WebAssembly) |
| **Stack** | Rust (axum, SQLite), Leptos → WebAssembly, wasmtime para extensões |
| **Site** | — (repositório: https://github.com/iramarfalcao/forjabase) |
| **Monetização** | Nenhuma: código aberto, MIT |
| **Próximo marco** | 0.1: SQLite, coleções base com CRUD de registros, filtros e superusuário |
| **Data-alvo** | A definir; 1.0.0 só quando estável (PLANO §6) |

## O que já está pronto

- Workspace com 4 crates, testes passando, painel compilando para WASM.
- Documentação: [FUNCIONALIDADES](FUNCIONALIDADES.md), [PLANO](PLANO.md), [GUIA](GUIA.md).

## O que falta

Tudo marcado 📋 em [FUNCIONALIDADES.md](FUNCIONALIDADES.md), na ordem do PLANO §5.
