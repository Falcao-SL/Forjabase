# Dependências de terceiros

## `htmx.min.js`

- Versão: **2.0.11**
- Origem: <https://unpkg.com/htmx.org@2.0.11/dist/htmx.min.js>
- Licença: BSD 2-Clause — <https://github.com/bigskysoftware/htmx/blob/master/LICENSE>
- Embutido no binário com `include_bytes!` e servido em `/_/htmx.min.js`.

Atualizar: trocar o arquivo, atualizar a versão acima e a constante em
`src/lib.rs`, e rodar os testes do painel (`cargo test -p forja-server`).
