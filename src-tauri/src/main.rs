// Sin consola en release: es un widget, no una herramienta de linea de comandos.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mideck::run()
}
