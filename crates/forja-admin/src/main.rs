//! Painel administrativo do Forja, em Rust compilado para WebAssembly (Leptos).
//! Compilar: `trunk build --release` nesta pasta. Por enquanto, só a casca.

use leptos::prelude::*;

#[component]
fn App() -> impl IntoView {
    view! {
        <main>
            <h1>"Forja"</h1>
            <p>{concat!("Painel administrativo · versão ", env!("CARGO_PKG_VERSION"))}</p>
        </main>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
