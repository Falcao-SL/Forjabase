# Status — Forja

> Atualizado em **2026-10-23**. Este arquivo é a fonte única de "em que pé está".

| Campo | Valor |
|---|---|
| **Estágio** | **Em desenvolvimento** — 0.1 entregue: coleções base e auth, autenticação completa e painel administrativo (login e CRUD de registros) na API do PocketBase |
| **Plataformas** | Servidor: macOS, Linux, Windows (Rust); painel: navegador, HTML no servidor + htmx (sem build de front-end) |
| **Stack** | Rust (axum; SQLite), maud + htmx, wasmtime para extensões |
| **Site** | — (repositório: https://github.com/iramarfalcao/forjabase) |
| **Monetização** | Nenhuma: código aberto, MIT |
| **Próximo marco** | 0.2: campo `relation`, cascata, `expand`, coleções `view` e macro `@collection.*` |
| **Data-alvo** | A definir; 1.0.0 só quando estável (PLANO §6) |

## O que já está pronto

- Workspace com 4 crates e 32 testes passando.
- SQLite é o único motor suportado (um arquivo por projeto, WAL). Turso e
  PostgreSQL continuam compilando atrás de feature flag, sem suporte e fora do CI.
- Superusuário (Argon2id, JWT), coleções base e auth, registros com filtro do
  PocketBase convertido em SQL com parâmetros, ordenação, paginação, regras e
  campos ocultos.
- Autenticação de usuários: login, refresh, verificação de e-mail, redefinição
  de senha, troca de e-mail, personificação e macros `@request.auth.*`.
- Painel em `/_/`: login por cookie `httpOnly`, lista de coleções, tabela com
  filtro e paginação via htmx, criar/editar/apagar registros com erros por campo.
- Cliente Python do PocketBase funciona contra o Forja (conferido em 2026-10-21).
- Documentação: [FUNCIONALIDADES](FUNCIONALIDADES.md), [PLANO](PLANO.md), [GUIA](GUIA.md),
  [ARQUITETURA](ARQUITETURA.md), [PARIDADE](PARIDADE.md).

## O que falta

Tudo marcado 📋 em [FUNCIONALIDADES.md](FUNCIONALIDADES.md), na ordem do PLANO §5.
